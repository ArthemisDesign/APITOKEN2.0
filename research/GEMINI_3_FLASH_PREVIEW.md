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

- Implementation readiness: `wire-fix candidate`; sandbox and non-sandbox host matrices both
  failed, so the signed 2.4.3 User-Agent is the next isolated post-deploy experiment.
- Terms/compliance: no new OAuth identity or scope; the existing Gemini subscription review still
  governs. Undocumented subscription transport evidence is not treated as vendor permission.
- Credential verdict: unchanged and within the existing encrypted roster threat model.
- Usage/settlement verdict: official paid rates and all existing token/Search dimensions are exact;
  successful generation must still prove terminal non-zero `usageMetadata` on the subscription wire.
- Streaming verdict: implementation reuses the proven incremental SSE translator, but this exact
  model still needs a live incremental frame and terminal usage.
- Quota verdict: owned discovery proves both bounded quota rows exist, not that generation works.
- Main unresolved risk: the current host plus signed User-Agent may still reject this owned plan.
  That result requires testing the minimal current-client header set once, then withdrawing the
  model rather than guessing a private alias.

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
| `generateContent` | non-sandbox daily Code Assist origin, public wire id, `requestType=agent` | route-owned project/session/request identity | 2xx candidate, canonical public `modelVersion`, terminal non-zero usage | existing 4xx/429/5xx policy; retry only before public bytes |
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
| Installed Antigravity | signed macOS app 2.4.3, bundle `com.google.antigravity`, Team ID `EQHXZ8M8AV` | official distributed artifact; non-normative | language server contains the public model and `daily-cloudcode-pa.googleapis.com`, but neither the agent alias nor sandbox origin | separate official artifact | no; static inspection only |
| CLIProxyAPI | `41fc5e134631789e98137245f576680c7fb9b322` | MIT; independent implementation | `internal/runtime/executor/antigravity_executor{,_request}.go`: non-sandbox daily is first generation origin; required request headers are Authorization, Content-Type and current Antigravity User-Agent | yes | no; source inspection only |
| antigravity-oauth-proxy | `410e825a23d0181469bf4062e7cebfced2b81440` | no repository license; evidence only, no code reused | `internal/antigravity/{constants,client}.go`: only non-sandbox daily generation origin; Authorization, Content-Type and Antigravity User-Agent | yes | no; source inspection only |

The two independent implementations corroborate only the origin/header hypothesis, not model
availability or the private quota alias. Public searches still did not find two unrelated maintained
implementations of `gemini-3-flash-agent`; the gateway therefore does not use it as a generation ID.

## Wire audit after the failed baseline

The first implementation mixed evidence from different Antigravity generations: it discovered the
model in signed Antigravity 2.4.3, but sent it through the older production sandbox origin and pinned
2.2.1 header tuple. The post-deploy matrix then showed a decisive split: all `countTokens`
preflights succeeded, while `minimal`, `low`, `medium`, `high`, SSE, cache, audio and tool generation
all returned the same Google `404 NOT_FOUND` model-resource error before any immutable turn.

Header classification:

- `Authorization: Bearer …` and `Content-Type: application/json` are required authentication/wire
  headers and remain server-owned.
- `User-Agent: antigravity/hub/<version> darwin/arm64` identifies the client release. Production
  still pins 2.2.1; signed 2.4.3 and CLIProxyAPI show that this value needs a separate controlled A/B,
  but it is not changed together with the origin so the experiment remains attributable.
- `x-goog-api-client` and `client-metadata` are not proven model selectors. Existing models work
  with them, while both independent implementations omit them from generation. They stay unchanged
  in the origin experiment and cannot be blamed merely because they differ from current clients.
- `Accept: text/event-stream` is load-bearing only for `streamGenerateContent`; it cannot explain
  matching non-stream 404s.
- Model access is primarily determined by OAuth identity/plan, endpoint deployment, top-level
  `model`, `requestType`, `project` and session/request identity. The public model ID and
  `requestType=agent` are corroborated; the sandbox deployment is the first isolated mismatch.

The safe experiment order is therefore: non-sandbox daily origin with the existing header tuple;
that retained 404, so the next deployed A/B updates only the signed User-Agent version. Only if it
also remains 404 may a later experiment compare the minimal current-client header set. This avoids
changing host and headers simultaneously and falsely attributing success.

## Controlled live results

| Date | Plan / client | Public / native model | Operation | Status | Incremental? | Usage non-zero? | Quota evidence | Sanitized result |
|---|---|---|---|---|---|---|---|---|
| 2026-08-02 | owned Google AI Pro / Antigravity | public preview / quota rows | model quota discovery | success | n/a | n/a | both agent and non-agent rows present | quota presence only; generation unknown |
| 2026-08-02 | same owned opaque profile | public id unchanged on sandbox daily origin | countTokens preflight plus four thinking levels, SSE, cache, audio and tool prompt | countTokens 2xx; all generation 404 NOT_FOUND | no public frame | no; zero immutable turns and zero spend | both rows still present | endpoint/model resource mismatch before generation |
| 2026-08-02 | same owned opaque profile | public id unchanged on non-sandbox daily origin, existing 2.2.1 UA | same controlled matrix | countTokens 2xx; all ten bounded generation legs 404 NOT_FOUND | no public frame | no; zero immutable turns and zero spend | both rows still present | origin alone is not the selector; current-UA A/B required |
| current-UA pending | same owned opaque profile | same host/id/wrapper, signed 2.4.3 UA | one micro generation first; expand only after success | pending | pending | required | exact profile attribution required | next isolated experiment; paid smoke capped at $0.0001 actual target |

The live runner must resolve exactly `gemini_oauth_000001` as the only healthy profile and use
`--models gemini-3-flash-preview`, require canonical calibration request attribution, and retain
only the sanitized report. No paid leg is retried without authoritative `not_started` proof.

## Conflict log

| Topic | Official | Owned live | Chosen behavior | Risk / next experiment |
|---|---|---|---|---|
| generation ID | official CLI sends public preview id | sandbox countTokens accepts it; sandbox generation returns model-resource 404 | keep public id unchanged | test the current signed origin before any private alias |
| generation origin | no public normative subscription contract | signed 2.4.3 and two independent implementations use non-sandbox daily; sandbox and corrected-host live generation are 404 | keep only this preview on non-sandbox daily | current-UA micro-smoke after deploy |
| request headers | no normative private header contract | 2.2.1 works for older models but both preview host matrices return 404; signed client is 2.4.3 | update only Preview to signed UA; leave working models unchanged | if still 404, test one minimal current header set, then withdraw |
| quota identity | no normative private row contract | agent and non-agent rows exist | Antigravity agent admission uses only `gemini-3-flash-agent` | explicit zero may reveal a changed row; withdraw/fix on evidence |
| subscription availability | public Developer API model exists | quota and countTokens exist, sandbox generation does not | candidate, not GA evidence | non-sandbox 404 requires allowlist withdrawal or a separately proven header correction |
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
