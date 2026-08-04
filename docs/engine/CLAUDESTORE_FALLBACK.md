# ClaudeStore — emergency fallback for the Claude and GPT planes

Claude transport status: **implemented / default-off / production live pending**. GPT
transport status: **implemented / default-off / blocked until a separate Codex-tier credential and live gate**.
Neither transport is part of the local subscription pools, is published as a separate
provider or model, and both are off by default. Their only role is the last compatible attempt
after a terminal result of the normal local pre-byte rotation of their own provider plane.

## Applicability boundary

- The client request, model, tariff, and the single internal money/request identity stay in the original
  Claude or GPT plane; no new public provider/model/catalog appears.
- Local subscriptions always take priority. A single account/network/5xx/429 continues the normal
  local rotation/retry policy and by itself does not permit the external call.
- The external attempt is permitted only before the first public byte and only after a
  `local pool exhausted/unavailable` result. After the first byte, replay or switching transports is forbidden.
- Authoritative terminal Anthropic/OpenAI usage is metered by the existing exact metering of its own
  plane and closes the original reserve exactly once. The GPT transport fails closed on zero or
  internally contradictory terminal usage.
- The external turn creates no local subscription quota/calibration observation, affinity, or health
  attribution for any specific local account/profile.
- The secret is read only by `crates/server/src/config.rs`, is never returned through API/metrics/logs, and
  is stored only in the production secret env. The production URL is compile-fixed; an arbitrary upstream cannot
  be set from the environment.
- Each switch is accepted only as a strict `0|1|false|true`; enabled without its own valid
  `sk-cs4-*` secret, or not on its own fixed provider plane, stops startup. Claude uses
  `CLAUDE_API_CLAUDESTORE_{FALLBACK_ENABLED,API_KEY}`, GPT uses the separate
  `CLAUDE_API_CLAUDESTORE_CODEX_{FALLBACK_ENABLED,API_KEY}`. Keys must not be reused:
  ClaudeStore switches a universal key between the Basic/Claude and Codex tiers.

## Claude Messages capability manifest

| Field | Evidence | State / decision |
|---|---|---|
| Product | `official`, 2026-08-03 | ClaudeStore is an independent pay-as-you-go API gateway, not a Claude subscription provider |
| Credential | `official`, 2026-08-03 | API key in `x-api-key`; plaintext is permitted only in the secret env |
| Native endpoint | `official` + unauthenticated live, 2026-08-03 | `POST https://api3.claudestore.store/v1/messages`; unauthenticated `/v1/models` answers with a bounded 401 `Missing API key` |
| Anthropic version | `official`, 2026-08-03 | `anthropic-version: 2023-06-01` |
| Non-stream | `official`; authenticated live `unknown` | Anthropic Messages JSON with terminal `usage.input_tokens/output_tokens` |
| Streaming | `official`; authenticated live `unknown` | SSE `message_start` → deltas → `message_stop`; the mock confirms the absence of post-byte replay, but incrementality and terminal usage of the live service are not yet verified |
| Tools | `official`; authenticated live `unknown` | Documentation claims standard Anthropic `tools`; fail-closed is preserved by the current wire validation |
| Models | `official` catalogue; key-scoped live `unknown` | The fallback does not rewrite the model id; an unknown/unavailable model terminates the single external attempt, while externally a sanitized local terminal response remains |
| Upstream quota | `official` | 429 + `Retry-After`; not used as Claude subscription quota evidence |
| Billing | `official`; authenticated live `unknown` | ClaudeStore deducts Anthropic-equivalent credits; customer settlement remains on the local Anthropic rate card and terminal usage |
| Data | `official`, policy v3.0 | Prompt/response content is claimed not to be retained after the request cycle; usage metadata is retained for 12 months |
| Rollback | `decision` | Remove/clear the secret env or disable the strict boolean; the local pool keeps working without the external dependency |

## Claude Messages wire and errors

| Operation | Contract | Runtime decision |
|---|---|---|
| Messages | `POST /v1/messages`, `x-api-key`, `anthropic-version`, the original Anthropic body | Forward bytes without OpenAI transliteration; private local subscription headers are not sent |
| Stream | Anthropic SSE | Use the existing `TeeMeter`; only one external attempt is possible before the first public byte, replay after it is forbidden |
| 400/401/403/402 | terminal client/credential/balance failure | Do not retry; hide ClaudeStore credential/balance details and return the already computed local terminal response |
| 429 | external capacity/rate limit, optional `Retry-After` | Do not retry and do not record Claude subscription cooling/quota; externally the local terminal response remains |
| 5xx/network before bytes | external transport fault | Do not retry; externally the local terminal response remains, no cascading to other external services |
| malformed/EOF after bytes | post-byte stream failure | No replay; settlement follows the existing conservative missing-usage policy |

After any started external `send`, failure is considered execution-ambiguous: the client gets the
sanitized local terminal status/body and a refund, but `x-apitoken-execution-state:
not_started` is stripped. Therefore the router cannot start another billable continuation on false
evidence; only a complete pre-external local terminal would have preserved this proof.

## GPT/Codex capability manifest

Full dated evidence dossier: [`research/CLAUDESTORE_GPT_FALLBACK_EVIDENCE.md`](../../research/CLAUDESTORE_GPT_FALLBACK_EVIDENCE.md).

| Field | Evidence | State / decision |
|---|---|---|
| Credential | `official`, 2026-08-03 | A separate `sk-cs4-*` universal key, switched in the dashboard to the ClaudeStore Codex tier; a Basic/Claude key is unusable |
| Native endpoint | `official` + unauthenticated live, 2026-08-03 | `POST https://api3.claudestore.store/v1/responses`, Bearer auth; without a key the endpoint and `/v1/models` return a bounded 401 |
| Public adapters | `implementation` | The original `/v1/responses`, `/v1/chat/completions`, and the Anthropic skin of the GPT plane converge into the internal Responses turn; the external transport always uses only `/v1/responses` |
| Models | `official`; authenticated live `unknown` | Compile-fixed allowlist: `gpt-5.5`, `gpt-5.4`; live `/v1/models` does not extend it automatically |
| Streaming | `official`; authenticated live `unknown` | Documentation claims Responses SSE; the mock verifies decoding and the absence of replay, but a real incremental stream is not yet proven |
| Usage | `official`; authenticated live `unknown` | Terminal OpenAI `usage` with nonzero input/total and a consistent sum is required; otherwise the attempt counts as failed |
| Tools/reasoning/structured output/Fast | `official` partially; live `unknown` | The dormant transport preserves the internal Responses body; enablement is blocked until a controlled matrix of all actually sellable controls |
| Local identity | `implementation` | `chatgpt-account-id`, `originator`, `client_metadata`, OAuth credential, proxy, or the private local upstream slug are not sent; the public model id is restored before send |
| Accounting | `implementation` | Existing Codex reserve/settlement and the local OpenAI tariff; a ClaudeStore turn writes no ChatGPT quota, affinity, or calibration evidence |
| Rollback | `decision` | Disable the Codex switch/remove the separate secret; the local ChatGPT pool keeps working |

## GPT/Codex wire and errors

| Operation | Contract | Runtime decision |
|---|---|---|
| Responses | `POST /v1/responses`, `Authorization: Bearer`, JSON Responses body, SSE | At most one attempt after the terminal local rotation policy; compile-fixed origin and model allowlist |
| Chat Completions / Anthropic skin | Public adapters of APIToken.sale | Use the shared internal turn; no separate ClaudeStore `/chat/completions` or `/messages` calls are made |
| 400/401/402/403/429/5xx/network | External terminal failure | Do not retry, do not change local home health/quota; return the original local status with a bounded body and without the `not_started` proof |
| Output started | Responses SSE delta | Neither the local nor an external attempt is started again; post-byte replay is forbidden |
| Terminal usage missing/zero | Not enough authority for exact settlement | Do not count as success and do not write calibration; the activation gate must prove nonzero usage before enablement |

The GPT fallback deliberately does not replace the Codex provider at startup with an empty sealed roster: it is an emergency
transport for a working subscription pool, not a standalone provider plane. The OpenAI
runtime constructor still requires at least one valid local profile.

## Written permission

The current [Terms and Conditions](https://claudestore.store/terms-and-conditions/) version
`v3.0-2026-07-23`, clause 8.2, prohibit reselling/redistributing/sublicensing API access to third parties without
the explicit written consent of ClaudeStore. Client fallback traffic falls into this zone.

On August 3, 2026 the operator received from a ClaudeStore administrator explicit written permission
for APIToken.sale to use the ClaudeStore key as a backup upstream for processing client
requests and for redistribution of API access. The operator keeps the original correspondence and the sender's
identity outside Git; the screenshot, personal data, and credential are not copied into the repository. This grant
lifts the clause 8.2 blocker for the stated scenario but does not replace the technical live gates below.

## Evidence and open live gates

Official sources, reviewed 2026-08-03:

- [LLM-readable service index](https://claudestore.store/llms.txt) and
  [full reference](https://claudestore.store/llms-full.txt) — canonical API3 base URL, the split of
  Basic/Claude and Codex tiers, Anthropic/OpenAI surfaces, and the pay-as-you-go product identity.
- [Messages API](https://claudestore.store/docs/api-reference/messages/) — request/response fields,
  `x-api-key`, usage, and the claimed Anthropic SDK compatibility.
- [Streaming](https://claudestore.store/docs/api-reference/streaming/) — Anthropic SSE event shape.
- [Errors](https://claudestore.store/docs/api-reference/errors/) and
  [Rate limits](https://claudestore.store/docs/guides/rate-limits/) — 4xx/5xx/529 and 429
  `Retry-After`; a stable RPM/TPM is not published.
- [OpenAI & Codex endpoints](https://claudestore.store/docs/api-reference/codex/) — a separate
  Codex-tier key, Bearer auth, `/v1/responses`, `/v1/chat/completions`, `/v1/models`, the models
  `gpt-5.5`/`gpt-5.4`, claimed SSE and terminal OpenAI usage.
- [Privacy Policy](https://claudestore.store/legal/privacy/) `v3.0-2026-07-23` — request-cycle
  content handling and 12-month usage metadata retention.
- The site references GitHub `zerofeesclub/claudestore`, but as of the review date the link answers
  `Repository not found`. Therefore there is no independently inspectable implementation SHA: the official docs
  remain the wire authority, and any discrepancy counts as an explicit evidence conflict, not code confirmation.

The following remain mandatory before serving:

1. plane-specific secret provisioning outside git with confirmed owner/mode and a kill switch; GPT
   requires a new separate key on the Codex tier, which is not present in the current task;
2. a bounded authenticated live matrix for each transport: supported model list, minimal
   non-stream generation with terminal usage, a real incremental SSE, deterministic 4xx,
   insufficient-balance/429, and a secret/privacy scan; GPT additionally verifies tools, reasoning,
   structured output, and Fast or explicitly excludes unproven controls;
3. a post-deploy smoke on the exact watchdog-green SHA verifying a single settlement and zero
   local subscription calibration attribution.

The Claude mock matrix already pins down: healthy local → 0 external attempts; local retry success → 0;
empty pool → exactly 1; external 5xx → local terminal + refund; a successful response → customer
settlement without local subscription attribution; post-byte SSE failure → error tail without replay.
The GPT mock matrix pins down: healthy local home → 0 external attempts; terminal local pool → exactly
one `/v1/responses`; local identity does not leave; the `gpt-5.5`/`gpt-5.4` allowlist; terminal usage
is mandatory; local calibration unchanged; a failed external attempt preserves the local HTTP status
but strips `not_started`. These tests, the build, and the merge by themselves do not close the authenticated
live gates and do not mean GA.
