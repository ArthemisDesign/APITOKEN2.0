# ClaudeStore GPT fallback — evidence dossier

## Review metadata

- Provider / slug: ClaudeStore API3 / `claudestore-codex-fallback`
- Review date and timezone: 2026-08-03, Europe/Moscow
- Product repository SHA: implementation commit containing this dossier; base
  `85d930792e3767c24d2e37f92c6aa81fd2be6a9c`
- Intended public API surface: existing APIToken.sale GPT `/v1/responses`,
  `/v1/chat/completions` and Anthropic-compatible skin; no new model or provider catalogue entry
- Intended credential source: separate root-owned `sk-cs4-*` key switched to ClaudeStore Codex tier
- Owned live plans available for test: none; the existing Basic/Claude key is intentionally excluded

## Executive verdict

- Implementation readiness: `research-complete`
- Terms/compliance verdict: operator supplied written ClaudeStore administrator permission for
  fallback/redistribution on 2026-08-03; original identity and correspondence remain outside Git
- Credential verdict: `blocked` until a separate Codex-tier key is provisioned
- Usage/settlement verdict: mock-proven; authenticated terminal OpenAI usage remains unverified
- Streaming verdict: officially documented and mock-proven decoder; authenticated incrementality
  remains unverified
- Quota/calibration verdict: ClaudeStore quota contract is unknown and intentionally never enters
  local ChatGPT calibration
- Main unresolved risks: model/control availability, terminal usage shape, incremental SSE,
  ClaudeStore-specific debit rate, external balance/rate-limit behavior and data-path live evidence

## Official sources

| Claim | URL | Page/schema version | Accessed | Exact proof | Does not prove |
|---|---|---|---|---|---|
| Codex base/auth/endpoints/models/usage | https://claudestore.store/docs/api-reference/codex/ | page `dateModified=2026-08-03` | 2026-08-03 | Base `https://api3.claudestore.store/v1`; Bearer `sk-cs4-*` on Codex tier; `/responses`, `/chat/completions`, `/models`; `gpt-5.5`, `gpt-5.4`; standard usage | Authenticated availability, exact SSE fields, every optional control or quota |
| Codex tier separation | https://claudestore.store/llms-full.txt | live text, no immutable version | 2026-08-03 | Universal key must be switched to ClaudeStore Codex tier; same three endpoints and two model ids | That a Basic/Claude key can be used concurrently or that a newly provisioned key is funded |
| Resale restriction | https://claudestore.store/terms-and-conditions/ | `v3.0-2026-07-23` | 2026-08-03 | §8.2 requires explicit written consent for resell/redistribute/sublicense | Identity/scope of the operator's off-repository grant |
| Data handling | https://claudestore.store/legal/privacy/ | `v3.0-2026-07-23` | 2026-08-03 | Provider privacy contract for request processing and retained usage metadata | End-to-end OpenAI/upstream retention or live regional route |

## Subscription plan and model matrix

| Exact plan | Public model/capability | Official status | Live status/date | Native route | Usage authority | GA decision |
|---|---|---|---|---|---|---|
| ClaudeStore Codex tier | `gpt-5.5` Responses/Chat | documented | unavailable, 2026-08-03 | `POST /v1/responses` | terminal OpenAI usage, unverified | dormant; blocked on owned key and live matrix |
| ClaudeStore Codex tier | `gpt-5.4` Responses/Chat | documented | unavailable, 2026-08-03 | `POST /v1/responses` | terminal OpenAI usage, unverified | dormant; blocked on owned key and live matrix |
| Basic/Claude tier | GPT generation | explicitly wrong tier | not tested | none | none | excluded; must not reuse production Claude fallback key |

## Wire matrix

| Operation | URL/query | Required headers | Body wrapper/control | Framing | Terminal usage | Errors/retry evidence |
|---|---|---|---|---|---|---|
| Model discovery | `GET https://api3.claudestore.store/v1/models` | `Authorization: Bearer` | none | JSON | n/a | unauthenticated live 401 on 2026-08-03; authenticated catalogue unknown |
| Responses generation | `POST https://api3.claudestore.store/v1/responses` | Bearer, JSON content type, SSE accept | standard Responses body, `stream:true` | officially SSE; exact live sequence unknown | required by implementation; official standard usage claim | unauthenticated live 401; runtime makes no external retry |
| Chat Completions | `POST /v1/chat/completions` | Bearer | standard Chat body | JSON/SSE documented | standard OpenAI usage documented | not called by fallback; public adapter translates to internal Responses turn |
| Quota/balance | unknown | unknown | unknown | unknown | unknown | activation blocker for operational diagnosis, never local quota authority |
| Health/refresh | no separate public contract | API key only | none | n/a | n/a | no OAuth/refresh flow; startup does not probe or spend |

## Model translation and controls

| Public id | Native id | Plan | Reasoning/speed/media controls | Price schedule | Quota scope | Evidence | Decision |
|---|---|---|---|---|---|---|---|
| `gpt-5.5` | `gpt-5.5` | Codex tier | Responses body preserves requested controls; authenticated support unknown | existing APIToken.sale OpenAI tariff; ClaudeStore debit rate unverified | ClaudeStore account, unknown windows | official model/endpoint only | compile allowlist; no private ChatGPT slug crosses boundary |
| `gpt-5.4` | `gpt-5.4` | Codex tier | same | existing APIToken.sale OpenAI tariff; ClaudeStore debit rate unverified | ClaudeStore account, unknown windows | official model/endpoint only | compile allowlist; no live-catalogue auto-expansion |

## Official rate card

| Effective epoch | Model/tier/region | Usage leg | Exact official unit/rate | nanoUSD representation | Overlap rule | Source |
|---|---|---|---|---|---|---|
| unknown | ClaudeStore Codex | ClaudeStore credit debit | documentation says published OpenAI token rates; exact effective table not captured | unknown | never treated as local customer-price authority | Codex API reference |
| existing product schedule | APIToken.sale `gpt-5.5`/`gpt-5.4` | customer settlement | unchanged repository metering schedule | existing integer nanoUSD | original request tariff snapshot wins | local metering authority; no price change in this patch |

## Native quota/credit contract

| Bucket | Duration/reset | Unit/scale | Measurement resolution | Hard-stop signal | Native credit rate | Evidence |
|---|---|---|---|---|---|---|
| ClaudeStore Codex balance | unknown | provider credits | unknown | expected 402/403, authenticated evidence absent | unknown exact table | official billing prose only |
| ClaudeStore rate limit | unknown | unknown | unknown | expected 429, authenticated evidence absent | n/a | general ClaudeStore error/rate-limit docs, not Codex live |

## Authentication threat model

- OAuth/device grant and official client identity: none; static API key only.
- Issuer/audience/scopes/redirect/state/PKCE: not applicable. The provider-side tier is an account
  control and must be confirmed as Codex before activation.
- Refresh rotation and blue-green race: no refresh. Both OpenAI slots may overlap during cutover and
  read the same root-owned key; one-shot request semantics and existing registry fencing remain.
- Stable duplicate identity: no local profile/account id is sent. Bearer key is the only provider
  identity.
- Proxy/geography coupling: fallback uses the host's direct egress; provider regional behavior is
  unverified and must be included in live validation.
- Secret fields and AEAD/AAD: secret is not stored in repository or credential envelopes; it lives
  only in mode-0600 `server.env`, is redacted by config `Debug`, and never appears in metrics/logs.
- Revocation/removal: set `CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0`, remove/rotate the key,
  then perform the normal watchdog-controlled OpenAI cycle.
- Auth Bot seller path and crash rollback: not applicable; this is not a subscription provider or
  Auth Bot roster member.

## GitHub implementation evidence

| Repository | Commit SHA | License | Last activity | Relevant paths | Concrete hypothesis | Independent? | Executed? |
|---|---|---|---|---|---|---|---|
| `https://github.com/zerofeesclub/claudestore` | unavailable | unknown | unknown | none | official site advertises source, but `git ls-remote` returned `Repository not found` on 2026-08-03 | no | no |

No inspectable independent implementation was found or used. The public API contract is simple
enough to implement from provider documentation, but the missing source gives no extra GA evidence.

## Controlled live results

| Date | Plan/region/client | Public/native model | Operation | Status | Incremental? | Usage non-zero? | Quota movement | Sanitized result |
|---|---|---|---|---|---|---|---|---|
| 2026-08-03 | unauthenticated / host egress | n/a | `GET /v1/models` | 401 | n/a | n/a | unknown | bounded JSON response, 61-byte body; body not retained |
| 2026-08-03 | unauthenticated / host egress | `gpt-5.5` | minimal `POST /v1/responses` | 401 | no | no | unknown | bounded JSON response, 61-byte body; body not retained |
| pending | owned Codex-tier key | `gpt-5.5`, `gpt-5.4` | model list + minimal generation + SSE + controls | blocked | unknown | unknown | unknown | separate key not yet supplied |

## Conflict log

| Topic | Official | Live | OSS | Chosen behavior | Risk | Next experiment / blocker |
|---|---|---|---|---|---|---|
| Key reuse | universal key must be switched to Codex tier | current Basic key intentionally not tested | none | separate env/credential only | accidental switch would disable Claude fallback | provision a new Codex-tier key |
| Models | only `gpt-5.5`, `gpt-5.4` documented | authenticated catalogue unknown | none | compile-fixed two-model allowlist | live account may expose fewer models | authenticated `/v1/models`, then minimal generation each |
| Responses SSE | supported | unauthenticated only | none | shared strict decoder, no retry after output | event/usage drift could produce partial failed stream | capture sanitized incremental event classes and terminal usage |
| Optional controls | broad OpenAI compatibility claimed | unknown | none | preserve request but keep feature dormant | unsupported field may make fallback fail | one controlled case per sold control/tier |
| Rates | published OpenAI rates claimed | debit movement unknown | none | customer tariff unchanged; no local calibration | provider cost can diverge | measure exact credit movement around minimal turn |

## Unsupported surfaces

- Every GPT id except exact `gpt-5.5` and `gpt-5.4`: no official contract in the reviewed source.
- ClaudeStore `/v1/messages` for GPT: official Codex page explicitly says it is not served on this
  tier.
- Standalone operation with zero local Codex homes: fallback is not a provider replacement.
- Provider quota calibration, affinity, account health, dynamic model discovery and automatic
  allowlist expansion: no trustworthy contract.
- Production activation, Fast guarantee, tools, reasoning, structured outputs and image input until
  each claimed surface passes the authenticated live matrix.

## Implementation decisions traceability

| Decision | Evidence rows | Product paths/tests | Rollback |
|---|---|---|---|
| Separate strict default-off key/switch | key tier rows, auth threat model | `crates/server/src/config.rs`; config unit test; systemd provider fences | disable switch/remove key |
| Fixed API3 Responses transport | official base/endpoints; wire matrix | `crates/forward/src/codex/claudestore.rs`; gateway mock | disable switch |
| Two-model allowlist | official model rows | fallback allowlist + negative model test | disable switch or remove model in reviewed patch |
| One attempt only after local terminal | runtime design, no provider retry evidence | Codex runner integration tests and metrics | disable switch |
| No local identity/calibration attribution | threat model; unknown native quota | captured mock wire and empty calibration report | disable switch |
| Terminal usage fail closed | usage official claim, live unknown | missing-usage integration test | disable switch; no GA until live proof |
| Preserve local public status but remove `not_started` | external execution ambiguity | API error unit test | disable switch |

## Secret-hygiene confirmation

- [x] no bearer/refresh token, cookie or credential file captured;
- [x] no raw email/account/subject/project/organization id retained;
- [x] no authenticated proxy URL retained;
- [x] no private error body or prompt content retained;
- [x] no temporary clones/captures created; inaccessible Git remote was queried read-only;
- [x] all saved results use bounded classes and synthetic input.
