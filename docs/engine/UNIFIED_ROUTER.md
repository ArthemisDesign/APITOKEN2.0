# UNIFIED_ROUTER — a single endpoint for all providers (target architecture)

Status: **stages 1–5, phases 6.1–6.3, policy/presets 6.4a–6.4b and the
telemetry/mock-load part of 6.4c are implemented with fallback disabled.**
`router.apitoken.sale` serves the entire public native contract through blue-green
processes `claude-router@8800/8801` behind an atomically switched Caddy backend, the
unified aggregated catalog `GET /v1/models{,/{id}}`, and universal
Chat/Responses/Messages lanes with model-based dispatch to three planes.
`/v1/messages/count_tokens` uses the same dispatch.
What remains to reach full OpenRouter-grade routing is post-deploy live canary and a separate 6.4c GA flag. This document fixes the target
picture, the public contract, the invariants, and the staged plan; each stage, when
implemented, updates this document and the adjacent instructions in the same commit.

## Context and goal

The product goal is to replicate the OpenRouter model (one account, one key, one
balance, a unified catalog of models from multiple providers, pay-as-you-go), but
**without losing quality for harness agents** (Claude Code, Codex, Gemini clients).
OpenRouter pushes every request through a single OpenAI-compatible format and
inevitably clips provider specifics (thinking signatures, Anthropic beta fields,
encrypted reasoning, stored responses). Our difference: heavy harnesses get not a
translation but the provider's real API.

| | OpenRouter | This solution |
|---|---|---|
| One key / balance / catalog | yes | yes |
| Universal OpenAI-compatible entry | yes | yes (universal lane) |
| Native fidelity for Claude Code / Codex | no, everything is translated | yes (native lanes) |
| Unsupported parameters | silently ignored | fail-closed `400 unsupported_parameter` |
| Provider preferences / fallback | yes | fallback 6.2 + durable fencing 6.3 + policy/preferences/presets 6.4b + telemetry/mock-load 6.4c are ready default-off; live canary and the unit flag remain |

The key fact that makes the solution cheap: the three provider planes are already
independent at the process level and already share one fenced PostgreSQL billing
authority — `sk-pool-…` keys work on all planes (see `docs/engine/ARCHITECTURE.md`,
`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`).

## Target architecture

```
                    router.apitoken.sale — the new single entry point
                    (added ALONGSIDE; the old domains are not disabled,
                     see "Migration policy")
                                    |
                    +---------------+----------------+
                    |           ROUTER               |  stateless replicas
                    |  auth passthrough · catalog    |
                    |  route planner · IR translation|
                    +-------+---------------+--------+
                            |               |
              +-------------+--+   +--------+---------+
              |  NATIVE LANES  |   | UNIVERSAL LANE   |
              | (100% fidelity)|   | (client coverage)|
              +-------+--------+   +--------+---------+
                      |                     | translation via
          +-----------+-----------+         | typed canonical IR
          v           v           v         v
     Anthropic    OpenAI      Gemini   selects any plane
      plane        plane       plane     by model ID + policy
     8787/8788   8793/8797   8795/8799
          |           |           |
          +-----------+-----+-----+
                            v
              BILLING CORE (unified, already exists)
     fenced PostgreSQL: keys · reserve/settle · ledger ·
     versioned pricing catalog · owner_epoch fencing
                            |
                            v
              Subscription pools behind each plane
        (OAuth envelopes, cooling, affinity, blue-green)
```

### Native lane

The input protocol matches the backend — the request passes without translation,
byte-for-byte in body, SSE, and provider-native errors. The router only authorizes
(key passthrough), resolves the model ID, and forwards the request to the stable
origin of the corresponding plane. This is the primary entry for harness agents and
the guarantee of protocol-level quality.

### Universal lane

A client that only speaks OpenAI Chat Completions (Aider, Continue, Roo/Kilo, Hermes,
most IDE plugins) sends any model from the catalog to `/v1/chat/completions`. The
router translates through a typed canonical Turn/Event IR into the native protocol of
the selected plane. The IR must cover: system/developer messages, content blocks and
images, tool calls and tool results, structured output, thinking/reasoning,
prompt-cache boundaries, usage, and canonical streaming events.

The translation contract is **strict, fail-closed**:

- a capability unsupported by the target plane → a clear `400 unsupported_parameter`;
  never silently drop `strict`, `thinking`, server tools, or response schema;
- opaque reasoning artifacts (Claude thinking signatures, OpenAI encrypted reasoning,
  Gemini thought signatures) carry provider provenance: they are returned only to the
  same provider or rejected; silent removal is forbidden (it breaks the agent loop or
  leaks internal reasoning).

## Public contract

One hostname; the input endpoint determines the wire protocol and (in stages 1a–1b)
the plane:

```
POST /v1/messages                                   Anthropic Messages (Claude Code)
                                                    (stage 5.1 — + any openai/* catalog
                                                    model via model-based dispatch into
                                                    the Anthropic Skin on Codex plane;
                                                    stage 5.2 — + any google/* catalog
                                                    model on Gemini plane)
POST /v1/messages/count_tokens                    Anthropic token counting with
                                                  model-based dispatch: native Anthropic,
                                                  local Codex counting, or native Gemini
                                                  `:countTokens` (stages 5.1–5.2)

POST /v1/responses                                OpenAI Responses (stage 1a — OpenAI
                                                  plane only, stage 4.1 — + any Claude
                                                  catalog model via model-based dispatch)
POST /v1/responses/input_tokens                   OpenAI token counting (openai-only for now)
GET  /v1/responses/{id}                           stored response — openai/* only
GET  /v1/responses/{id}/input_items               (decision 5)

POST /v1/images/generations                     OpenAI Images (GPT Image 2) — native OpenAI
                                                lane, one non-streaming PNG
POST /v1/images/edits                           one-to-five-reference PNG edit — native OpenAI lane

POST /v1/chat/completions                         universal OpenAI-compatible entry
                                                  (stage 1a — OpenAI plane only,
                                                   stage 3.1 — + any Claude catalog model
                                                   via model-based dispatch,
                                                   stage 3.3 — + Gemini models)

GET  /v1/models                                   unified aggregated catalog (stage 1b)
GET  /v1/models/{id}

GET  /v1beta/models                               Gemini native
POST /v1beta/models/{id}:generateContent
POST /v1beta/models/{id}:streamGenerateContent    (alt=sse and alt=json)
POST /v1beta/models/{id}:countTokens

GET  /balance                                    bodyless shared-authority read: Anthropic →
                                                  OpenAI → Gemini only after transport,
                                                  response-header timeout, or 5xx; 401 and any
                                                  non-5xx are terminal
GET  /health, /live, /ready                       router-local process probes
```

The four universal POST paths (`chat/completions`, `responses`, `messages`, and
`messages/count_tokens`) accept an optional `models: [<id>, …]` as a continuation
chain after the mandatory `model`, strict OpenRouter-shaped `provider` preferences,
and reserved primary IDs `preset/auto|quality|fast|hermes`. These capabilities are
active only with `CLAUDE_ROUTER_FALLBACK_ENABLED=1`; a default-off router returns a
lane-shaped `400` before touching catalog/policy/plane. The router expands the
preset, takes one aggregate snapshot, does canonical dedup, provider
filters/order/reviewed sort, `allow_fallbacks`, then an engine-owned account-policy
preflight; `models` and `provider` never reach the plane. The detailed contract and
retry matrix are in `docs/engine/ROUTING_FENCING.md` §§3.3, 5.

The `/api/v1` prefix (OpenRouter-compatible paths) is **not** added in the MVP:
Cline, Codex, and most custom-provider configurations accept their own Base URL. It
will be needed only if clients hard-bound to the OpenRouter path appear.

## Harness-agent compatibility

| Harness | Required contract | Entry |
|---|---|---|
| Claude Code | Anthropic Messages, SSE, open beta/header/body lists | native Anthropic lane; Anthropic Skin (stage 5) for non-Claude models |
| Codex | Responses API; custom provider supports only `wire_api="responses"` | native OpenAI lane; a chat-only proxy is insufficient |
| OpenCode | OpenRouter preset or custom AI SDK / OpenAI-compatible provider | universal lane, namespaced model IDs |
| Cline | OpenRouter with custom Base URL, OpenAI Compatible, or Anthropic with custom Base URL; custom headers for GPT Fast | universal lane or native Messages for Claude |
| Hermes | OpenRouter / custom providers; routing, fallback, auxiliary models; context ≥ 64K (smaller windows are rejected at startup) | universal lane; preset on the router, preset catalog — models ≥ 64K only |
| Aider, Continue, Roo/Kilo, most IDE agents | OpenAI Chat Completions | universal lane |
| Native SDKs | Messages / Responses / Gemini Developer API | the corresponding native lane |

The verified OpenAI-compatible sample covers two different client classes. OpenCode
1.18.11 (`@ai-sdk/openai-compatible` 2.0.41), Kilo 7.4.17 (2.0.48), Cline 3.0.49
(2.0.63), and Roo Code 3.54.0 (2.0.28) use a single AI SDK transport; its unknown
model options go into the JSON verbatim. Continue 1.5.47, Hermes 0.19.1 (OpenAI
Python SDK 2.24.0), and Aider 0.86.2 (LiteLLM) verify independent OpenAI-compatible
implementations. Codex, Claude Code, and Gemini CLI in this same matrix deliberately
use native Responses, Messages, and Gemini Developer API rather than the Chat skin.

The controlled production matrix of real harness clients is launched manually only
(every successful generation is billed):

```bash
APITOKEN_API_KEY=... bash tests/router_harness_live_matrix.sh
```

The script brings up a loopback evidence proxy for each case: the real key is given
only to the proxy via env and is immediately removed from its own environment; Cline,
Continue, OpenCode, Kilo, Codex, Claude Code, Gemini CLI, Hermes, and Aider see only
a placeholder. Evidence files contain only mode-0600 metadata: endpoint, protocol,
model, status, request/response tier, and SSE/control types; credential/general-header
values, prompts, tool arguments, and generated content are never written to evidence.
Each client's transient state is isolated in a single-use temp-root and removed by a
trap. For a targeted rerun, pass a list of labels via
`APITOKEN_HARNESS_CASES=cline-fast,codex-fast`. Harness-internal retries are allowed,
but every accepted executable attempt must yield HTTP 200 with the expected tier; Fast
requires an authoritative response `service_tier=priority`. Claude Code additionally
proves the full Messages SSE lifecycle and the current
structured-output/context/cache controls; Codex — the Responses lifecycle and current
tool forms; Gemini CLI — native `streamGenerateContent`. A separate
`opencode-gemini-tools` case runs OpenCode without user plugins/sanitizer, requires a
real bash call and a second Chat turn, and bounded evidence confirms the raw AI SDK
`$schema`/exclusive bounds, the tool-call response, and the replayed tool history.
`opencode-claude-native`, with the same clean provider config without plugins, runs
the main Claude 4.6 and the stock title agent on Claude 4.5; evidence requires both
HTTP 200 and confirms that the raw `reasoning_effort: low` title request reached the
router without client-side rewrite.
`opencode-claude-effort-xhigh` and `opencode-claude-effort-max` separately run Opus 5
without user plugin/request rewrite via real `--variant xhigh|max`; evidence requires
HTTP 200 and confirms the raw `reasoning_effort` of each level at router ingress.
`opencode-fast` and `kilo-fast` run without plugin/request rewrite: the config
declares a separate Fast model with the original API model ID and the
models.dev-style option `serviceTier:"priority"` customary for such configs. The AI
SDK passes the unknown option into the JSON verbatim; the router accepts this bounded
camelCase alias on GPT Chat/Responses, strips it, and passes the Codex plane the
canonical `service_tier:"priority"`. Evidence requires exactly the raw ingress
`serviceTier`, the absence of a client-side canonical body, and an authoritative
response `usage.service_tier:"priority"`; OpenCode additionally verifies the `low`
reasoning variant. This fixes a whole transport class rather than one specific
OpenCode user config. Cline and Continue verify the header selector, Aider and Codex
— the canonical snake_case body. Hermes Fast is verified through the documented
`providers.<name>.extra_body.service_tier`: in Hermes 0.19.1 the interactive `/fast`
is supported, but the separate `--oneshot` path does not pass `agent.service_tier` to
the created `AIAgent`, so the headless matrix does not attribute to the router an
option the client lost and uses its native custom-provider body contract.

OpenCode does not project arbitrary fields of the OpenAI `/v1/models` response
directly into its internal model schema: the canonical config-plugin
`packages/opencode-router-plugin/apitoken-router.js` converts
`data[].apitoken.pricing.standard` into the stock `model.cost` by dividing the exact
nanoUSD/M by `1e9` to the USD/M number OpenCode requires. The synthetic GPT Fast model
with the original API ID uses `pricing.priority`; Standard uses `pricing.standard`.
Such discovery must run synchronously with the credential of the current launch: a
shared file cache is acceptable only for pricing-free capability metadata, not for
the full key-scoped response.

The plugin always requests the live catalog first. Its last-good fallback stores only
a strict whitelist of model ID/name, limits, reasoning efforts, and service tiers:
the AES-256-GCM snapshot is cryptographically bound to the exact credential/base URL
pair, is written atomically with mode `0600`, has a schema guard, a 15-minute
freshness TTL, and a 7-day maximum stale age. A different key/URL, an unknown
version, wrong permissions, corruption, and expiry are rejected fail-closed. On
fallback every model explicitly gets the suffix `[stale metadata; pricing
unavailable]`, the warning contains the snapshot time, and `cost` is entirely absent
until the next successful live discovery. This way a brief catalog outage neither
leaves OpenCode without models nor can show another key's rate or a stale personal
price. The contract and crypto/negative tests live next to the plugin in
`packages/opencode-router-plugin`. Router limit fields remain independent, but OpenCode 1.18 accepts
its native `model.limit` only when both `context` and `output` are present. The adapter therefore
keeps validated partial limits in the encrypted capability record while omitting `model.limit` from
both live and stale OpenCode cards until both required values are authoritative. It never invents an
output ceiling, and one model with partial metadata cannot prevent OpenCode from starting.
The plugin entrypoint has exactly one ESM export — a default factory: OpenCode
1.18.11 interprets every export of the file as a separate plugin and rejects named
constants/helpers with `Plugin export is not a function`. The exact export shape is
pinned by a unit test and a client smoke.

The generated-image capability is deliberately not advertised in the OpenCode model
schema. The package provider uses `@ai-sdk/openai-compatible` 2.0.41: its Chat
decoder accepts `message.content` only as string/null and does not consume native
Gemini `inlineData` or OpenRouter-style image metadata. Therefore even an image model
gets `modalities.output:["text"]`, otherwise the UI would promise a result the
transport will drop. This does not disable image generation in the gateway:
`google/gemini-3.1-flash-image` keeps working through native Gemini
`generateContent`/`streamGenerateContent`, returning
`candidates[].content.parts[].inlineData` with the usual authoritative settlement.

After live discovery
OpenCode itself computes message cost from input/output/reasoning/cache token usage;
a separate `usage.cost` from the router is not required. Its custom-provider schema
supports a separate `context_over_200k`, so the plugin fills it only at the exact
provider threshold of 200000; the arbitrary GPT threshold 272000 and the separate
Anthropic cache-write 1h are not expressible in the stock OpenCode 1.18.11 cost
schema, but the router keeps them in `apitoken.pricing.long_context` and
`cache_write_1h` for clients with a more precise metering model.

The control run of 2026-08-02 is green on Cline 3.0.49, Continue CLI 1.5.47, OpenCode
1.18.11, Kilo 7.4.17, Codex CLI 0.146.0, Claude Code 2.1.220, Gemini CLI 0.53.1,
Hermes 0.19.1, and Aider 0.86.2: the base 19 executable cases are green, including a
real multi-turn OpenCode→Gemini bash tool cycle and a clean OpenCode Claude
main/title cycle without request rewrite. The current matrix contains 21 executable
cases after adding two Opus 5 effort cases (Gemini CLI — Standard native; the
remaining regular harnesses — both tiers).
Roo Code 3.54.0 is installed and has compatible OpenAI base URL/model/service-tier
settings, but the extension has no official headless CLI, so it is honestly marked
`SKIP` rather than simulated through another client.

Critical Claude Code requirements (native Anthropic lane contract):

- do not buffer SSE — buffering the full response stalls the client;
- preserve `/v1/messages?beta=true` and pass `anthropic-beta` / `anthropic-version`
  to the plane verbatim;
- headers and body fields are open lists: unknown fields are proxied, not rejected
  and not dropped;
- do not wrap native Anthropic errors: Claude Code sometimes recovers based on the
  error text;
- `GET /v1/models?limit=1000` — no redirects and faster than three seconds;
- Claude Code ignores discovery IDs that do not start with `claude` or `anthropic`
  (therefore `anthropic/claude-*` is compatible); `/v1/messages/count_tokens` is
  optional — without it the client counts context locally; model discovery is off by
  default and requires `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1` (Claude Code
  v2.1.129+).

Codex: a full Responses API is required, not an adaptation of Chat Completions —
custom provider supports only `wire_api="responses"` (this is the default when
omitted). Codex 0.146 passes function, Lark custom, and client-executed `tool_search`
in top-level `tools`; the Codex plane accepts these forms with the same bounded
parser as legacy `additional_tools` and returns client tool calls without executing
them on the gateway. Hosted `web_search` is not a client tool and is rejected
fail-closed, so the harness's isolated custom-provider profile disables it
explicitly.

Namespaced IDs from the aggregated catalog are an executable contract, not just
discovery metadata: the router preserves the universal request body, so each plane
strips its own prefix before admission (`anthropic/`, `openai/`, `google/`). For GPT
Fast on Responses and Chat, the canonical contract uses `service_tier:
"fast"|"priority"`; the compatible models.dev/AI SDK alias `serviceTier:
"fast"|"priority"` is accepted by the router only here and normalized into the
canonical body. An Anthropic Messages harness may send native `speed: "fast"`, while
`service_tier: "fast"|"priority"` is accepted as a compatible alias. All variants
normalize to the effective `priority`, which determines reserve, settlement, and the
public `usage.service_tier`. `GET /v1/models`, after the usual key check, returns the
backend-native overlay `{models: []}` for the Codex `originator`/User-Agent: Codex
merges it with the built-in catalog; regular OpenAI/OpenRouter SDKs still receive the
aggregated `{object:"list",data:[…]}`.

A harness that can set custom headers but not arbitrary body fields (in particular,
Cline) selects GPT Fast with the `x-apitoken-service-tier: fast` header (the
`priority` alias is also accepted). The router allows it on executable GPT requests
of Chat, Responses, and Messages, normalizes it into the body
`service_tier:"priority"` before admission, and always strips the header itself
before the plane. The camelCase body alias `serviceTier` serves harnesses on
`@ai-sdk/openai-compatible`, which serialize models.dev-style options verbatim; it is
allowed only on Chat/Responses. For both aliases the model is resolved by the catalog
first; an explicit fallback chain must consist entirely of `openai/*` models.
`messages/count_tokens`, `serviceTier` on Messages, a non-GPT model, a
repeated/invalid header, and contradictory `serviceTier`/`service_tier` or Messages
`speed` get a lane-shaped `400` before any billable call. A missing canonical body
field and equivalent `fast`/`priority` are compatible; the plane always receives
`service_tier:"priority"`, and the camelCase alias and the capability header are
stripped. Reserve, settlement, and effective-tier evidence still belong to the Codex
plane — the router only adapts client input.

## Invariants

1. **Billing lives only in the provider plane.** The router neither reserves nor
   charges money. The client's key is passed to the plane verbatim; `request_id`,
   reserve → delivering → settle, and the exactly-once ledger remain the plane's
   responsibility (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`). Double charging is
   ruled out by construction.
2. **The router never retries an ambiguous outcome that may have reached the plane.**
   Repeating after a timeout once the request was sent would create a new
   `request_id` and a second charge: the backend may have executed the request and
   settled even if the router never received the response. Continuing to the next
   model of an explicit fallback chain is allowed only after a proven TCP
   ConnectionRefused or an exact non-2xx `x-apitoken-execution-state: not_started`,
   with which the plane guarantees the absence of a charge. 401/402 and client 4xx
   are not retried; signed 429 is the capacity exception. Details — "Fallback
   semantics".
3. **No shared execution queues, semaphores, or circuit breakers in the router.**
   Concurrency limits, breakers, and cooling live in the planes (process isolation,
   see `docs/engine/ARCHITECTURE.md`). The router adds no global limit — otherwise an
   overloaded plane would eat the capacity of the others. Router readiness is never a
   conjunction of all planes' health; there are no synchronous health checks on the
   request path. The single exception is the fail-fast 64 MiB memory admission for
   materialized universal request bodies: a known size is rounded up in 1 MiB steps;
   an unknown/chunked size starts with one unit and fail-fast acquires more units as
   bytes are actually read. Admission never waits in a queue, is bounded by a
   15-second idle and a 5-minute absolute body-read deadline, and does not hold the
   native/SSE response body.
   The single-model path releases the permit after the outbound upload; advanced
   fallback holds it until terminal response headers, while the parsed template is
   still needed for the next attempt. The data plane has no router-owned
   response-header deadline: a non-stream Chat/Responses/Messages plane may return
   headers only after a long generation legitimately completes. Lifetime after
   connect is governed by client disconnect and the plane; the two-second header
   deadline exists only on the safe read-only `/balance` failover.
4. **SSE is never buffered**, neither in the router nor in the Caddy in front of it
   (a Claude Code gateway protocol requirement). A client disconnect transitively
   tears down the router→plane connection so the existing TeeMeter drain can finish
   reading authoritative usage and settle correctly.
5. **Money is integer-only** (bigint / nanoUSD strings) on all new surfaces.
6. **The old per-provider domains** (`api.`, `openai.api.`, `gemini.api.apitoken.sale`)
   remain full production entries for the entire migration period — not "emergency
   fallbacks" but live endpoints of active clients. Their contract, behavior, and SLA
   do not change; see "Migration policy".
7. **The router is a separate bounded context** (`crates/router`), talks to the
   planes only over HTTP, and does not import `pool`/`forward`. The Control API is
   loopback-only account/pricing management; it does not participate in the router's
   data plane.

### Early auth and the request-body memory boundary

Universal model dispatch must read the JSON request body for `model`, but has no
right to materialize up to 32 MiB from an unauthenticated client. The producer-first
contract on each fixed plane is a bodyless `POST /internal/router/auth/preflight`:
the forwarding admin is checked with the same in-memory `authed`, a customer
credential with the same active-key `AsyncBilling` resolver as live admission. The
endpoint is read-only, does not read the prompt, neither reserves nor charges money,
does not read pricing policy, and returns only:

- `200 {"schema_version":1,"authenticated":true}`;
- `401 unauthorized` for a missing, unknown, or inactive credential;
- `503 auth_unavailable` when the billing authority is unavailable.

Public provider vhosts do not route `/internal/*`; the router talks only to stable
loopback origins. The consumer keeps a two-second timeout per probe and uses fixed hedged
order Anthropic → OpenAI → Gemini. Anthropic starts immediately; OpenAI starts after
50 ms without a conclusive result and Gemini after another 50 ms, while an inconclusive
response that leaves no useful active probe launches the next origin immediately. The
first exact schema-v1 success or terminal 401 wins; transport/404/5xx/malformed
responses are inconclusive. Thus a healthy fast Anthropic authority normally avoids
secondary authority work, while a hung origin cannot impose its full timeout before a
healthy later authority is tried. Neither success nor the credential is cached between
requests. Outstanding request futures are dropped after a conclusive result, but this
does not guarantee cancellation of provider DB work already accepted. Contradictory
200/401 mean a violation of the shared authority; on the wire the first conclusive
outcome received wins, and actual provider-plane admission re-checks the credential
before reserve anyway. The deployment-only startup probe remains an eager concurrent
probe of all three origins.

After auth the consumer makes a fail-fast reservation against the 64 MiB budget in
1 MiB steps. A valid `Content-Length` is rounded up; a chunked/unknown size first
gets 1 MiB and acquires more weight only when crossing the next MiB boundary. On
exhaustion the router immediately returns a lane-shaped 503 with no queue and no
billable call; a body without progress for 15 seconds or unfinished after 5 minutes
gets 408 and releases its units. In the single-model path the parsed JSON is dropped
before the network, and the permit is handed to the outbound body and released after
upload/cancellation. Advanced routing keeps one parsed template for subsequent
attempts, so it honestly holds its permit until terminal response headers; an open
SSE response does not hold a permit. A billable data-plane attempt has no separate
response-header or body timeout: this keeps long non-stream and streaming generations
semantically identical and creates no ambiguous local abort after execution has
started. A client disconnect cancels the wait. A bounded header timeout remains only
on read-only `/balance`, where failover cannot create a second execution or charge.

## Migration policy (soft migration)

The product has active clients on the existing per-provider endpoints, so the
migration is soft only, with no single "old shutdown" date:

- **Nothing is disabled.** `api.apitoken.sale`, `openai.api.apitoken.sale`, and
  `gemini.api.apitoken.sale` keep serving traffic indefinitely — at minimum until a
  separately announced deprecation program with a measured share of residual traffic
  and personal communication with affected clients. A sunset date is deliberately
  absent from this document.
- **The unified endpoint is a new, separate hostname.** Existing domains are neither
  reused nor changed in behavior: `api.apitoken.sale` remains the direct Anthropic
  entry. New clients and new integrations get the unified domain; old clients migrate
  voluntarily when it suits them.
- **Same backend.** Both entries lead to the same provider planes and one billing
  authority, so a client's key, balance, and ledger are identical on any entry —
  client migration is a base URL change, not an account migration.
- **Public documentation leads with unified.** `apitoken.sale/docs` (portal, API
  reference, integration builder, models & pricing, `/md/*`, llms.txt) presents
  `https://router.apitoken.sale` as the recommended entry: native lanes for each
  provider plus an OpenAI-compatible universal lane and a unified catalog; the
  per-provider hosts are described there as fully supported legacy entries with no
  sunset date. The same position is propagated to the other user-facing surfaces: the
  dashboard (provider cards), the landing page and FAQ, the marketing `/models`
  pages and integration guides, the `/docs/learn` cluster (4 locales), the error
  references (`/errors/[tool]`, `/md/docs/errors`), and the buyer-facing OpenKeys
  surfaces (key-issuance text, provider cards, setup commands) — everywhere the
  primary instruction points at the unified endpoint, and legacy hosts are mentioned
  only as supported entries for existing integrations.
- **New capabilities — unified first.** The universal lane, the unified catalog, and
  routing policy evolve on the unified endpoint; the old domains keep their current
  contract (critical fixes are of course shared — the planes are the same).
- **Observability is split.** Traffic metrics per hostname, so that a decision on any
  future deprecation program rests on the actual traffic share of the old domains,
  not on estimates.

## Models and catalog

The unified catalog publishes namespaced IDs: `anthropic/claude-*`, `openai/gpt-*`,
`google/gemini-*`, `kimi/*`. The namespace denotes the model family, not necessarily a single
executor: when alternative backends of one model appear (Anthropic direct, Bedrock,
Vertex), the route planner will be able to choose between them. A native ID is
advertised as an alias only while it is globally unambiguous. If two planes publish
the same alias, the router strips it from all conflicting entries and alias lookup
returns 404; the namespaced IDs of both models remain executable, and their private
native ID is still used for body rewrite and pricing preflight. Therefore a new model
cannot silently hijack an existing alias through plane ordering. Besides upstream
aliases, the router publishes ONE router-side alias: `claude-haiku-4-5` for the
upstream dated id `claude-haiku-4-5-20251001` (Anthropic lists haiku only with its
date, unlike opus-5, so the site-documented bare name is injected here). Requests
that arrive under the bare alias are rewritten to the dated native id before the
plane is called — the engine admission canonicalizes the dated id back to
`claude-haiku-4-5` (`crates/metering`), so both spellings settle identically.
Each plane response
is bounded by 4 MiB and 1,024 models; IDs and display names by 256 bytes with no
control characters or surrounding whitespace; a duplicate namespaced ID makes the
whole refresh malformed. An expired refresh has a separate per-plane singleflight:
waiting callers use both the successful and the failed/oversized result of the same
in-flight attempt, but the next independent request retries immediately with no
negative cache/circuit breaker. The 30-second TTL is deterministically skewed to
27/30/33/36 seconds for Anthropic/OpenAI/Gemini/KIMI so that one warm aggregate does not
create a synchronous refresh burst of all providers. An oversized/malformed producer
keeps last-good and marks the namespace degraded.

### Catalog planes vs protocol lanes

A **lane** is a wire protocol — envelope shape, path prefixes, SSE dictionary — and there are
exactly three. A **plane** is where a model list comes from, and there are four: KIMI needs its
own even though it speaks Anthropic Messages and therefore dispatches over the Anthropic lane.
The reason is the transparency invariant: `GET /v1/models` on the Anthropic plane is a
byte-for-byte proxy of `api.anthropic.com`, so appending our KIMI aliases to it would let every
client — including those who never asked for KIMI — tell our fleet from Anthropic's. Discovery
therefore moves to an internal producer, `GET /internal/router/catalog/kimi`, published beside
`/internal/router/catalog/pricing` on the same slot. The router reads it as the `kimi` plane and
routes `kimi/*` over the Anthropic lane; `CLAUDE_ROUTER_KIMI_ORIGIN` defaults to the Anthropic
origin because the gateway is composed into those same slots.

The KIMI plane is **optional**: a slot with `CLAUDE_API_KIMI_ENABLED` unset answers the producer
with an empty list, and a build predating the producer has no such route at all. Neither state
marks the catalog degraded for clients — absence of models we never advertised is not degradation
— but `claude_router_catalog_degraded_total{namespace="kimi"}` still moves, so a real outage
stays visible to us. The three mandatory planes keep the old behaviour: missing means degraded.

Only published subscription aliases are advertised (`k3`, `k3[1m]`, `k3-256k`,
`kimi-for-coding`, `kimi-for-coding-highspeed`). The official Open Platform ids
(`kimi-k3`, `kimi-k2.6`, `kimi-k2.7-code…`) are tariff keys the gateway refuses on the wire, so
neither the producer nor `/internal/router/catalog/pricing` resolves them. Every key is quoted for
`kimi/*` and served: the strict-policy concept that used to exclude KIMI was removed with the
retired pricing design on 2026-08-10.

A namespaced `kimi/<alias>` is resolved by the Anthropic plane's admission back to the bare
alias before dispatch (`KimiGateway::resolve_public_model`). The plane strips only its own
`anthropic/` prefix, so without that resolution a published `kimi/*` id matched no alias and went
verbatim to the Claude upstream.

Normalized `reasoning_efforts` and `service_tiers` are published both in
`apitoken.capabilities` and as the previous top-level mirrors for client
compatibility. Clients with model-level options (in particular, OpenCode/Kilo) can
use `service_tiers` to show a separate Fast model, keeping the original API model ID
and adding the canonical `service_tier:"priority"` or the compatible
`serviceTier:"priority"`; the Standard model and reasoning variants remain
independent. Product/model eligibility belongs to the existing versioned
multi-provider pricing catalog (`docs/engine/CONTROL_API.md`,
`crates/registry/src/pricing.rs`), and exact provider token rates belong only to
`crates/metering`.

Authoritative runtime metadata arrives producer-first from the planes. Anthropic
already publishes native `max_input_tokens`, `max_tokens`, and `capabilities`; owned
OpenAI/Gemini model resources add a closed expand-only object:

```json
{"apitoken":{
  "limits":{"context":400000,"input":272000,"output":128000},
  "capabilities":{"reasoning_efforts":["low","medium","high"],
                  "service_tiers":["standard","priority"],
                  "input_modalities":["text","image"],
                  "output_modalities":["text"],
                  "tool_calling":true,
                  "structured_outputs":true,
                  "streaming":true}
}}
```

The fields inside `limits` are independent: unknown input/context are omitted, but a
known output is preserved. Codex input is taken from the authenticated last-good
`/codex/models.context_window`, and when profiles disagree the minimum common
guarantee is published; missing metadata on even one serving profile lifts the
guarantee. The optional OpenAI `name` is taken only from provider-authored
`display_name` and is dropped on profile conflict. Gemini publishes configured native
limits and exact model-specific capabilities: the image-generation route accepts
text/image, outputs text/image, and explicitly has
`tool_calling:false`/`structured_outputs:false`; text routes output text and support
both controls; only Gemini 3 Flash Preview additionally advertises exact PCM WAV
audio input. The router consumer accepts token limits only as positive integers up to
`u32::MAX`, capability booleans (`tool_calling`, `structured_outputs`, `reasoning`,
`streaming`) only as JSON bool, modalities only without duplicates from
`text|image|audio`, and the other arrays only without duplicates from the closed sets
`none|minimal|low|medium|high|xhigh|max` and `standard|priority`. Anthropic
`max_input_tokens` becomes `limits.context` and `limits.input`, `max_tokens` becomes
`limits.output`, and native capabilities are normalized into the same
modalities/control booleans. `thinking.supported` becomes a separate `reasoning`, so
the presence of reasoning is not confused with the presence of an effort switch:
pre-4.6 Claude honestly has reasoning, but the adapter accepts only the model default
and therefore publishes an empty `reasoning_efforts`; Claude 4.6 allows
`low|medium|high|max`, Claude 4.7+ and 5 — `low|medium|high|xhigh|max`, after
intersecting with the current native capability catalog. For owned planes the absence
of a separate `reasoning` allows exact derivation from authoritative efforts (`none`
by itself does not enable reasoning). Missing legacy metadata is omitted without
guessing; malformed authoritative metadata makes the plane fetch failed, so the
router uses its last-good and sets `x-apitoken-catalog-degraded`, or omits the plane
if there is no last-good yet. The pricing overlay is added to the same `apitoken`
object and does not overwrite limits/capabilities. Limits must not be derived from
the model id, pricing thresholds, or client tables; capabilities must not be guessed
from namespace/`owned_by`.

The Images API models are published by the OpenAI plane on the same surface, under
exactly the two ids the paid image routes admit (`gpt-image-2` and its immutable
snapshot `gpt-image-2-2026-04-21`), so discovery can never name an id those routes
would refuse. They are the one catalog family whose serving reality is *not* the text
pool: they carry no upstream text-catalog intersection and no invented token limits,
their capability block is authoritative and negative almost everywhere
(`output_modalities:["image"]`, `reasoning_efforts:[]`, `tool_calling:false`,
`structured_outputs:false`, `reasoning:false`, `streaming:false`), and the additional
`apitoken.endpoints` array names `/v1/images/generations` and `/v1/images/edits`. A
client picks the endpoint by reading `output_modalities`; sending an image model to
`/v1/chat/completions`, `/v1/responses` or `/v1/messages` is a fail-closed `400` that
names the image routes, never a "model does not exist" `404` — the model does exist,
the endpoint is wrong. Their key-scoped card prices the three token classes an
OpenAI-compatible card can carry (text input, cached text input, generated output);
the separately audited *image*-input class metered on edits has no field in the closed
schema v1 card and is documented in `docs/commerce/PRICING.md` instead of being folded
into a leg that would then misprice plain generation.

`created:0` means only "the producer provided no shared OpenAI-compatible creation
date". The router does not substitute this field with a release date from
documentation or a model name. Active `preset/*` entries publish
`apitoken.routing.members` exactly from the live members available to this key, and
`variable_model_pricing:true`: the selected model is determined per request, so a
preset has no single rate. Their limits are the minimum over only the values known
for every active member; arrays are the ordered intersection; a boolean capability
equals `true` only on unanimous true, `false` on any authoritative false, and is
omitted on a true/unknown mix. Manifest ranks/context remain reviewed routing
constraints but are not runtime authority for `/v1/models`.

The personal price projection uses a separate producer-first loopback contract
`POST /internal/router/catalog/pricing` on each fixed provider plane. The router
passes the customer credential verbatim and a bounded list of `(opaque catalog id,
provider, native model id)`; the engine applies the live legacy scalar or the exact
strict-policy payable multiplier to the audited tariff and returns only integer
`nano_usd_per_million_tokens` rate cards. Account/key/policy/rule identity, balance,
and settlement are not part of the response. The shared 30-second cache stores only
the model/capability catalog: personal rates are forbidden in it. The consumer calls
the pricing authority on every `GET /v1/models{,/{id}}`, validates
version/unit/canonical decimal strings and the exact ordered subset, removes models
outside the subset from the public list, and adds expand-only metadata to the allowed
ones:

```json
{"apitoken":{"pricing":{"unit":"nano_usd_per_million_tokens",
  "standard":{"input":"5000000000","output":"30000000000",
    "cache_read":"500000000","cache_write":"6250000000"},
  "priority":null,"long_context":null}}}
```

The full key-scoped `/v1/models` response must not be placed in a shared cache; all
responses of the surface get `Cache-Control: private, no-store`. A `401` from the
authority is terminal, while transport/non-2xx, malformed/mixed-version, or oversized
responses after iterating the fixed origins yield a public `503
pricing_unavailable`: a zero, last-good, or another key's rate card is forbidden.

Wire schema v1 is closed to unknown fields. One authority request contains
`schema_version:1` and no more than 256 unique candidates. A larger aggregated
catalog is cut by the router into deterministic chunks of 256, the exact ordered
subsets are merged in the original order, and the entire pricing overlay is rejected
if even one chunk failed transport/schema/auth validation:

```json
{"schema_version":1,"candidates":[
  {"id":"openai/gpt-5.6","provider_id":"openai","model_id":"gpt-5.6"}
]}
```

Success returns `mode:admin|legacy|strict`, the exact unit, and the ordered subset of
input IDs:

```json
{"schema_version":1,"unit":"nano_usd_per_million_tokens","mode":"legacy","entries":[{
  "id":"openai/gpt-5.6",
  "standard":{"input":"5000000000","output":"30000000000",
    "cache_read":"500000000","cache_write":"6250000000"},
  "priority":{"input":"10000000000","output":"60000000000",
    "cache_read":"1000000000","cache_write":"12500000000"},
  "long_context":{"threshold_tokens":272000,
    "standard":{"input":"10000000000","output":"45000000000",
      "cache_read":"1000000000","cache_write":"12500000000"},
    "priority":{"input":"20000000000","output":"90000000000",
      "cache_read":"2000000000","cache_write":"25000000000"}}
}]}
```

The example values illustrate full-price GPT and are not a separate price list of
this document. Anthropic `cache_write` means the regular 5m class and additionally
publishes the optional `cache_write_1h`; Gemini, having no separate write bucket,
returns `"0"`. `priority` exists only for GPT models that actually support Fast.
`long_context` preserves the provider threshold even when the client cannot account
for it. An unsupported/forbidden model is absent from the ordered subset. An invalid
request gets `400 invalid_request`, an unknown/inactive credential — `401
unauthorized`, an unavailable pricing authority — `503 pricing_unavailable`; none of
these outcomes is replaced by a zero or globally cached price.

`/v1/models` is the only path collision of the native planes: the unified endpoint
must aggregate the catalogs of all planes (cache, partial catalog when one plane is
down, without blocking the others). Catalog aggregation is precisely the first code
justifying `crates/router`.

## Fallback semantics and billing fencing

The naive rule "fallback only before the first byte" is insufficient: on a timeout
the backend may have executed the request and settled even if the router never
received the response. Hence the gradation:

- **Stages 1a–5: no cross-model fallback at all.** The only retry is the existing
  in-plane one before the first public byte (no-byte retry boundary); it is safe
  because it does not create a new billable request after delivery has begun.
- **Phase 6.1:** the planes emit the internal `x-apitoken-execution-state:
  not_started` only before started with a refund/cancel guarantee; the router strips
  it from all public responses.
- **Phase 6.2, MVP fallback:** the default-off `models` field defines a serial
  continuation after the mandatory `model`. The router preflight-validates the whole
  chain against one catalog snapshot and repeats only on the exact 6.1 signal or a
  proven TCP ConnectionRefused. Timeout, unsigned 5xx, abort after headers, and
  client 4xx fail closed.
- **Phase 6.3, durable fencing:** a shared execution group / attempt ID, idempotent
  reservations, and atomic selection of a single billable winner — reservation
  identity is extended from `request_id` to `(group_id, attempt_id)`, and the settled
  record admits exactly one winner per group (an extension of the current `UNIQUE
  ledger(kind, request_id)`). Implemented migration-first: Caddy strips client
  capability headers, the router injects one CSPRNG UUIDv4 per explicit fallback
  chain, the planes validate and durably store the pair, and registry
  loser-settlement forces zero-charge/full refund. Any loser increments an
  always-zero incident metric.
- **Phase 6.4:** provider preferences/presets and an engine-owned strict policy
  filter the chain before attempt 1. The router and fixed planes export bounded
  fallback/not-started counters; Prometheus scrapes them separately and links them
  with double-winner, balance divergence, and settlement detectors. Mock-load is
  green, but the production unit remains default-off until live canary of the exact
  deployed SHA.
- **Ambiguous disconnect → no automatic retry on another model.** The client gets an
  honest error and decides for itself; silent retry on timeout is a path to double
  charging.

## Universal lanes decisions (fixed 2026-08-01, before stage 3)

Discussed with the product owner and approved; the implementation of stages 3–6
follows them, and deviating requires revising this section.

1. **Translation lives in the planes, not in the router.** Universal entries are
   implemented by adapters inside each plane (stage 3: chat→Messages in the Anthropic
   plane, chat→generateContent in the Gemini plane; stage 4: Responses→native; stage
   5: Messages→native). The router gets exactly one new capability — model-based
   routing: parse the request body, extract `model`, choose the plane by namespace
   (`anthropic/`→8790, `openai/`→8792, `google/`→8794) or by alias from its own
   cached catalog; the body is then proxied unchanged, and the namespaced ID is
   resolved by the plane's admission. Translation in the router was rejected: it
   duplicates provider logic outside `forward`, detaches billing (reserve/settle)
   from the plane, and inflates the router into a second engine.
2. **No unified IR type.** The "canonical IR" from the staged plan means an event
   contract — a typed vocabulary (text delta, tool_call delta, reasoning delta,
   usage, finish) — that every per-plane adapter must reproduce and that is pinned by
   the plane's contract tests. A shared IR struct into which all providers are
   translated was rejected: it is a path to the lowest common denominator and silent
   loss of specifics (the OpenRouter scenario).
3. **Capability matrix + fail-closed with a defaults allowance.** Each plane has an
   explicit matrix of universal-entry parameters: honored / unsupported. An
   unsupported parameter with a non-default value → `400 unsupported_parameter`; with
   a default value it is accepted (stock SDKs send defaults in batches;
   compatibility is preserved). Unknown fields are proxied (open list). This
   legalizes the leniency of the existing `crates/forward/src/codex/chat.rs` as
   "lenient for defaults" and makes it the policy of all adapters.
4. **Reasoning.** `reasoning_effort` maps to the provider's native thinking config;
   the reasoning stream is delivered as deltas in the documented `reasoning_content`
   extension (the DeepSeek/OpenRouter convention). Signatures/encrypted reasoning are
   **not exposed** in universal lanes — a documented limitation: harness clients use
   native lanes. Therefore inbound `reasoning_content` is display-only: it must not
   be promoted to unsigned native thinking. If it is the only payload of an assistant
   turn, the adapter omits that turn and merges adjacent identical roles; a genuinely
   empty assistant without reasoning remains a 400.
5. **Stored responses (stage 4) — only for `openai/*`.** `store:true` and
   `GET/DELETE /v1/responses/{id}` work only for OpenAI models; for the rest →
   `400 documented_limitation`. A cross-provider response store is not built.
6. **Stage 5 mirrors 3–4.** `/v1/messages` for `openai/*` and `google/*` —
   Messages→native adapters in the corresponding planes with the same capability
   matrix; thinking deltas without signatures; replay of thinking blocks for
   non-Claude models is not supported.
7. **Stage 6: fencing and fallback are implemented in phases.** The foundation
   already exists in `crates/registry/src/pricing.rs` (versioned catalog, provider
   switches, account policy). Phases 6.1–6.3 implemented the internal `not_started`,
   the default-off serial fallback via `models`, and the durable execution
   group/single billable winner; policy/presets and telemetry/mock-load are
   implemented in phase 6.4; live canary and the unit flag remain. The detailed
   contract is `docs/engine/ROUTING_FENCING.md`.

## Existing base (what we reuse)

Verified by a code audit on 2026-08-01; everything listed really exists:

- Chat Completions on top of Responses (`crates/forward/src/codex/chat.rs`) —
  provider-specific; use as reference and a source of contract tests, without
  declaring it a universal IR without rework;
- typed dispatch of `response.*` streaming events
  (`crates/forward/src/codex/transport.rs`);
- retry only before the first public SSE event — on all three planes;
- disconnect drain to authoritative usage and settlement
  (`crates/forward/src/meter.rs`);
- per-model Gemini cooling (`crates/forward/src/gemini/pool.rs`);
- a unified `AffinityStore` with provider projections for Anthropic/OpenAI/Gemini
  (`crates/forward/src/affinity.rs`);
- fenced reserve/settle, `owner_epoch` fencing, and an exactly-once ledger
  (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`);
- versioned multi-provider pricing catalog and provider switches
  (`docs/engine/CONTROL_API.md`);
- Gemini native paths already served by the plane
  (`docs/engine/GEMINI_PROVIDER.md`).

Two clarifying caveats from the audit:

- the circuit breaker is code-global within a process
  (`crates/forward/src/breaker.rs`); per-provider isolation is achieved by the
  process model (one process = one plane), not by separate breaker objects. For the
  router architecture this is sufficient, but the phrasing "a provider has its own
  breaker" means deployment, not code;
- `ProviderMode::Combined` (`crates/forward/src/state.rs`) is a legacy rollout bridge
  for installations with old systemd units, not a "combined pool" and not the target
  model; it does not serve Gemini. The router does not use it.

## Staged plan

1. **1a. Caddy fan-in — IMPLEMENTED.** `router.apitoken.sale` routes by path shape to
   the existing loopback origins: `/v1/messages*` and `/balance` → 8790,
   `/v1/responses*` + `/v1/chat/completions` → 8792, `/v1beta/*` → 8794; `/health` is
   answered by Caddy itself (not a conjunction of plane health), other paths — 404.
   No new code; isolation, billing, and auth passthrough — out of the box. The
   provider is determined by the path, not by the key and not by the model.
   `/v1/models` is deliberately not served until stage 1b.
2. **1b. `crates/router` + unified catalog — IMPLEMENTED.** Stateless router
   (`crates/router`, binary `claude-router`, blue-green loopback slots
   `127.0.0.1:8800/8801`): byte-for-byte proxy of the three planes with no native
   retries and no shared timeout (streams are not clipped), hop-by-hop headers are
   stripped, errors are shaped to the path's lane. The unified `/v1/models`
   aggregates plane catalogs concurrently: namespaced IDs (`anthropic/…`, `openai/…`,
   `google/…`) + only globally unambiguous aliases, per-plane singleflight TTL cache
   27/30/33 s + TTL-less last-good. The router strictly normalizes producer-owned
   limits/capabilities into `apitoken` and compatible top-level mirrors: the
   Anthropic native model resource, owned OpenAI `apitoken` metadata, and owned
   Gemini `apitoken` metadata are the authority instead of router/client model-name
   tables. A cross-plane collision strips the alias from all conflicting entries;
   namespaced IDs remain available.
   A downed plane is omitted with the `x-apitoken-catalog-degraded` marker, an empty
   plane catalog counts as a failure, a plane 401/403 → a unified 401, all planes
   down without cache → 503. Auth passthrough unchanged; `/health`, `/live`,
   `/ready` — router-local. Deploy: the third tested artifact in the watchdog →
   promote → stage chain. `router-bluegreen.sh` starts the inactive slot, requires a
   direct `/ready`, an exact `/proc/<pid>/exe` from the selected immutable release,
   and a loopback-only `/startup` that receives the exact unauthenticated auth
   contract from at least one provider plane, after which the root-owned
   `router-promote.sh` atomically replaces `/etc/caddy/router-active.caddy`,
   validates, and reloads Caddy. New requests move to the target, and the old Axum
   process receives SIGTERM only after cutover and drains already-open SSE within
   `TimeoutStopSec=660`. At stage 1b — no cross-provider translation and no fallback;
   subsequent phases extend the same bounded context. The Caddy cutover is done: the
   vhost `router.apitoken.sale` terminates TLS and imports the same single-active
   backend as the stable loopback origin `127.0.0.1:8802` for metrics/checks. The
   legacy `claude-router.service:8798` is kept only as a bootstrap/rollback anchor of
   the first transition and is stopped and disabled after the switch. This eliminates
   both the 502 window and release-induced SSE aborts without adding multi-host HA
   and without changing default-off fallback.
3. **Universal Chat (2–4 weeks).** `/v1/chat/completions` for all catalog models:
   text, images, tools, structured output, streaming. Implemented per decisions 1–4
   of the "Universal lanes decisions" section: adapters in the planes, the router —
   only model-based routing, an event contract instead of an IR struct, a capability
   matrix. Subpackages: **3.0** — fixing the decisions in this document
   (IMPLEMENTED); **3.1** — router model-routing + Anthropic plane adapter (text,
   streaming, usage) — **IMPLEMENTED**: `POST /v1/chat/completions` in the router
   (`crates/router/src/chat.rs`) buffers only the request body (32 MiB — the ceiling
   of the largest plane), extracts `model`, and selects the plane by namespace prefix
   without querying the catalog or by alias through the cached catalog; the body is
   proxied unchanged, and dispatch errors (400 invalid JSON/no `model`, 404
   `model_not_found`, 503 `catalog_unavailable`, unified 401) come in the OpenAI
   envelope. The Anthropic plane adapter (`crates/forward/src/anthropic.rs`, a route
   in `ProviderMode::Anthropic`) translates chat→Messages (system/developer →
   top-level `system`, merging consecutive same-role messages,
   `max_completion_tokens`→`max_tokens`; if the client set no output cap, the
   mandatory Messages `max_tokens` equals the model's native ceiling (64k for Claude
   ≤4.5, 128k for 4.6+/5) and can be lowered only by the balance cap;
   `stop`→`stop_sequences`, `user`→`metadata.user_id`, stripping the
   `anthropic/` prefix before admission) and calls the shared `forward()` — auth,
   reserve, rotation, identity injection, tee-metering, and settle unchanged. The
   response is translated on the outside: Messages SSE → `chat.completion.chunk`
   (role/text/finish chunks, ping→heartbeat, `event: error`→OpenAI error frame
   without `[DONE]`, usage chunk per `stream_options.include_usage`), JSON message →
   `chat.completion` (usage includes cache tokens with
   `prompt_tokens_details.cached_tokens`).
   A successful SSE requires a valid Messages lifecycle `message_start` → block
   events → `message_delta` → `message_stop`; a malformed known event, a mismatching
   `data.type`, an impossible order, or premature EOF → an OpenAI protocol error
   without `[DONE]`. Unknown named events are ignored; the last unfinished SSE frame
   is parsed at EOF. Capability matrix: structured/reasoning/penalties/n>1/store and
   other non-default unsupported parameters → `400 unsupported_parameter` until stage
   3.4; default values are accepted, unknown fields are proxied. All errors of this
   path (including the plane's `local_err` and upstream passthrough) are converted to
   the OpenAI envelope with status preserved (402 LowBalance too) and `Retry-After`;
   **3.2** — tools/tool_choice + contract tests of the event vocabulary —
   **IMPLEMENTED**: chat `tools[]` and legacy `functions[]` → Messages `tools[]`
   (`parameters`→`input_schema`, a missing schema → `{"type":"object"}`);
   `tool_choice` (auto/required/none/named function) and legacy `function_call` →
   Messages `tool_choice` (auto/any/none/tool); `parallel_tool_calls:false` →
   `disable_parallel_tool_use:true`; defaults (empty `tools`, `auto`) are not
   inserted into the body. In history, assistant `tool_calls[]`/`function_call` →
   `tool_use` blocks (the `arguments` JSON string is parsed into `input`; a legacy id
   becomes the deterministic `callu_<name>`), role `tool`/`function` → a user message
   with `tool_result` blocks, and series of tool responses are merged into one user
   message (the Messages parallel-tool-calls semantics). In the response, non-stream
   `tool_use` blocks → `message.tool_calls` (`input` is serialized back into an
   `arguments` string, `content:null` when there is no text), SSE
   `content_block_start(tool_use)` → a tool_calls chunk with id/name,
   `input_json_delta` → arguments deltas; the tool ordinal is numbered separately
   from the Messages block index. Contract tests of the event vocabulary (decision
   2): tabular "canonical Messages event sequence → chunks" for text, single and
   parallel tool calls, text+tool, and usage — in the tests of
   `crates/forward/src/anthropic.rs`; e2e — `tests/universal_chat_smoke.sh` (the mock
   serves a tool_use dialog; checks of tools non-stream/stream/history and the
   end-to-end chain router→engine→mock); **3.3** — Gemini plane adapter —
   **IMPLEMENTED**: `crates/forward/src/gemini/chat.rs`, a route in
   `ProviderMode::Gemini`. Chat→GenerateContentRequest: system/developer →
   `systemInstruction`, user/assistant → `contents` with the Gemini roles user/model
   and merging of consecutive same-role messages,
   `max_completion_tokens`/`max_tokens` → `maxOutputTokens` only with an explicit
   client cap; on omission the shared Gemini admission uses the model's native output
   limit and lowers it only by balance; `stop` → `stopSequences` (≤5),
   temperature/top_p/top_k → `generationConfig`, stripping the `google/` prefix
   before admission. The adapter synthesizes an internal request to
   `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse` and calls
   the shared `gemini_api()` — admission, reserve, affinity, rotation, Code Assist
   wrapper, tee-metering, and settle unchanged. Tools: chat `tools[]`/legacy
   `functions[]` → `tools:[{functionDeclarations}]`. The shared bounded translator
   for Chat tools, Responses tools, Messages `input_schema`, and both
   structured-output surfaces translates legal JSON Schema into the exact Google
   `Schema` subset: local JSON Pointer `$ref`/`$defs` inline, string `const` into
   `enum`, numeric `const` into equal bounds, nullable union and representable
   exclusive/contains constraints — into native fields. Annotations are stripped;
   unrepresentable constraints and unknown keywords get a local 400 with the exact
   schema path instead of weakening the contract or an upstream `INVALID_ARGUMENT`.
   Expansion is bounded by 4096 nodes and depth 64; duplicate property names are data
   and are preserved. Missing parameters are omitted; `tool_choice`/legacy
   `function_call` → `toolConfig.functionCallingConfig` (auto is not inserted,
   required→ANY, none→NONE, named → ANY+allowedFunctionNames). History: assistant
   `tool_calls[]`/`function_call` → functionCall parts, role `tool`/`function` →
   functionResponse parts in user content (the name is recovered from the tool_call_id
   via the id→name map of the same history; an unknown id → 400; non-JSON tool output
   is wrapped as a string in `{result}`); series of tool responses are merged.
   Response: non-stream candidates[0] — text parts are merged, functionCall →
   `message.tool_calls` (args → an arguments string, synthesized ids
   `callu_<name>[_N]`, content:null without text), generated image-MIME `inlineData`
   parts → `image_url` content parts with data URLs (content becomes the
   array-form `[{type:"text"|"image_url"}]` — billed media is delivered, not
   dropped), finishReason → finish_reason
   (MAX_TOKENS→length, SAFETY/RECITATION/BLOCKLIST/PROHIBITED_CONTENT/SPII→
   content_filter), promptFeedback.blockReason without candidates → content_filter
   with empty content, usageMetadata → usage (completion = candidates+thoughts, cached
   → `prompt_tokens_details.cached_tokens`), model = `modelVersion` or the requested
   one. SSE: data-only GenerateContentResponse frames → a role chunk, content deltas,
   an image-MIME `inlineData` part → one content array delta with the image_url data
   URL, functionCall delivered whole as a single tool_calls chunk (there are no arguments
   deltas on the wire), finishReason → a finish chunk, the last usageMetadata → a
   usage chunk after the confirmed terminal state at EOF (per
   `stream_options.include_usage`) → `[DONE]`; `promptFeedback.blockReason` is also
   terminal evidence. Malformed JSON/known provider shape and EOF without both
   terminal signals → an OpenAI protocol error without `[DONE]`; a sanitized
   mid-stream `{error}` → an OpenAI error frame without `[DONE]`. Unknown extra JSON
   fields keep forward compatibility; the last data-frame without a terminating empty
   line is parsed at EOF.
   The capability matrix is the same 17 rules of the Anthropic plane plus
   `parallel_tool_calls` and `user` (19 total), and the plane's difference: a closed
   list of top-level fields (an unknown field → `400 unsupported_parameter`, because
   the Code Assist wrapper would otherwise drop it silently). Errors: the Google
   envelope `{error:{code,message,status}}` → the OpenAI envelope with status
   preserved (402 LowBalance too) and `Retry-After`; a special mapping of native
   `400 API_KEY_INVALID` → `401 authentication_error`. Replayed functionCall parts
   receive the confirmed Code Assist context-engineering `thoughtSignature` marker,
   so the next functionResponse executes statelessly and without opaque-signature
   passthrough; the response's real provider signatures remain hidden per decision 4.
   Contract tests — tabular ones in `crates/forward/src/gemini/chat.rs` (request,
   matrix, response, SSE); no e2e harness was added for the Gemini leg: the plane's
   native path is covered by its own tests, and the mock harness cannot do AEAD
   profile envelopes — the adapter seam is covered by unit/contract tests; the
   production live regression is `opencode-gemini-tools` in
   `tests/router_harness_live_matrix.sh`; **3.4a** — images + structured output —
   **IMPLEMENTED** (both planes): image_url parts of user messages — Anthropic:
   data: URL → base64 source, http(s) → url source (both native Messages image
   blocks); Gemini: only data: URL → inlineData (generateContent does not accept
   http(s) links — an honest 400; fileData requires a File API upload); `detail` !=
   auto is rejected with `400 unsupported_parameter` on both. `response_format`
   json_schema → Anthropic GA `output_config.format` (the wrapper
   name/strict/description are not proxied — only the schema; Messages has no
   json_object → matrix 400); on Gemini json_object → `generationConfig.responseMimeType:
   application/json`, json_schema → +`responseSchema` (the wrapper is likewise
   stripped, and the schema passes through the shared Code Assist supported-subset
   translator); **3.4b** — `reasoning_effort` → native thinking config +
   `reasoning_content` deltas (decision 4) — **IMPLEMENTED** (both planes): Anthropic
   inbound `reasoning_effort` accepts the compatibility set
   minimal|low|medium|high|xhigh|max (null/absent — off; any other non-null value →
   `400 invalid_request` with `param: reasoning_effort`) and maps to the GA
   `output_config.effort` (minimal is clamped to low, no beta header needed; `effort`
   coexists with `format` from 3.4a in one `output_config` without overwriting it).
   The exact matrix: Claude 4.6 accepts low|medium|high|max, Claude 4.7+/5 —
   low|medium|high|xhigh|max; a model-specific unsupported level gets a local 400
   before reserve/upstream. Gemini low|medium|high map to
   `generationConfig.thinkingConfig` (`thinkingLevel` is proxied as-is — the plane
   itself maps the level into the wire model id; `includeThoughts: true`).
   **3.4c** (a fix from live probes of the native lane): on Anthropic `effort`
   alone is not enough — on 4.6+ models adaptive thinking is off by default, and
   the default `display: "omitted"` sends thinking blocks with empty text, so with
   non-null `reasoning_effort` the adapter additionally injects `thinking: {"type":
   "adaptive", "display": "summarized"}` (a client's explicit `thinking` is not
   overridden — open list). On pre-4.6 models the upstream accepts neither
   `output_config.effort` nor adaptive thinking; a valid OpenAI-compatible effort
   there is a hint and degrades to the model default without both fields. A client's
   explicit legacy `thinking` is preserved. There is no separate metering modifier
   for effort: Anthropic counts thinking within the shared `output_tokens`, and the
   existing reserve bounds all output through `max_tokens`. The response — the
   `reasoning_content` convention: Anthropic thinking blocks and Gemini thought parts
   are merged into `message.reasoning_content` (non-stream; the field is present only
   with non-empty reasoning); thinking_delta/thought parts of the stream →
   `{"delta":{"reasoning_content": ...}}` chunks in the upstream's natural order
   (reasoning before content, role chunk first). Signatures are not exposed (decision
   4): signature_delta/thoughtSignature are dropped, redacted_thinking is ignored. On
   the next Chat request a non-empty `reasoning_content` is accepted as display-only;
   a reasoning-only assistant turn is omitted, while regular content/tool history is
   translated unchanged. This makes both planes' responses replay-safe for
   `@ai-sdk/openai-compatible` without forging provider signatures. The
   `reasoning_effort` rule was removed from both planes' capability matrix (Anthropic
   17→16, Gemini 19→18).
   The shared request-validation contract of Chat/Responses/Messages: missing and
   explicit `null` of an optional control mean absence/default, but any present
   non-null control is validated fail-closed before reserve/upstream. `stream` and
   `stream_options.include_usage` — JSON boolean only;
   `max_completion_tokens`/`max_tokens`/`max_output_tokens` — only a positive integer
   within `u64` range (zero, negative, fraction, string, compound value, and overflow
   → lane-shaped 400). For Chat aliases the first non-null spelling wins: `null` of
   the preferred alias allows the legacy fallback, a malformed preferred alias is
   terminal. OpenAI Chat/Responses return the exact failing spelling in
   `error.param`; the Anthropic Messages envelope keeps its stock format without
   `param`, with the control name in the message. The semantics are implemented by
   the shared `crates/forward/src/validation.rs` and separately pinned by wiring
   tests of Anthropic/Gemini Chat and Responses, Codex Chat and native Responses, and
   the Codex/Gemini Messages skin.
4. **Universal Responses for Codex parity (2–4 weeks).** `POST /v1/responses` for all
   catalog models: text, images, tools, reasoning, usage, streaming. Implemented per
   decisions 1–5 of the "Universal lanes decisions" section: adapters in the planes,
   the router — only model-based routing, stored responses — `openai/*` only (for the
   rest an explicit `400 documented_limitation`). The streaming wire contract is
   identical for the native OpenAI, Anthropic, and Gemini planes: every JSON object
   in `data:` contains a `type` matching the SSE `event:` field and a strictly
   increasing gapless `sequence_number`; the lifecycle events
   `response.created|in_progress|completed|failed` carry the Response object in the
   `response` field. Therefore stock OpenAI SDKs recognize the events without
   client-side wrappers. A terminal failure emits an `error` event (`code`, `message`,
   `param`), then `response.failed` with the full failed Response object; the comment
   keepalive sequence is not consumed. Subpackages: **4.1** — router dispatch +
   Anthropic plane adapter (core: text, usage, stream; tools in the request and
   function_call in the response) — **IMPLEMENTED**: `POST /v1/responses` in the
   router (`crates/router/src/responses.rs`) repeats the stage-3.1 chat dispatch —
   only the request body is buffered (32 MiB), `model` is extracted, the plane is
   selected by namespace prefix without querying the catalog or by alias through the
   cached catalog, the body is proxied unchanged, and dispatch errors (400 invalid
   JSON/no `model`, 404 `model_not_found`, 503 `catalog_unavailable`, unified 401)
   come in the OpenAI envelope. Stored endpoints (`POST /v1/responses/input_tokens`,
   `GET/DELETE /v1/responses/{id}`, `GET /v1/responses/{id}/input_items`) do NOT use
   dispatch and remain the native OpenAI lane (decision 5; token counting is also
   openai-only for now — a documented limitation). The Anthropic plane adapter
   (`crates/forward/src/anthropic_responses.rs`, a route in `ProviderMode::Anthropic`)
   translates Responses→Messages and calls the shared `forward()` (auth, reserve,
   rotation, identity injection, tee-metering, settle — unchanged). Request:
   `instructions` and system/developer items → top-level `system` (instructions
   first), an `input` string → a user message, an array of items (a message item —
   `{type:"message",…}` or a compact `{role, content}` without type) → messages with
   same-role merging, `input_text`/`output_text` parts → text blocks, `input_image` →
   image blocks (the same translation as the chat adapter: data: → base64, http(s) →
   url source, `detail` != auto → 400), `tools` → `tools[]`
   (`parameters`→`input_schema`, `strict` is stripped; a non-function tool →
   `400 unsupported_parameter`), `tool_choice`/`parallel_tool_calls` → Messages
   `tool_choice`, `max_output_tokens` → `max_tokens`; omission materializes the same
   native 64k/128k ceiling rather than a universal-lane default; `reasoning.effort` →
   the same model-specific `output_config.effort` matrix (minimal clamped to low) +
   injection of `thinking: {type:"adaptive", display:"summarized"}` (as in 3.4c; on
   earlier models the hint degrades to the model default; a client's explicit
   `thinking` is not overridden), `text.format` json_schema → `output_config.format`
   (the wrapper is stripped; json_object → 400), the capability matrix (`background`,
   `service_tier`, `truncation`, `include`, `prompt_cache_key`, `safety_identifier`,
   `user`, `metadata`, `max_tool_calls`, non-default `text.verbosity`) with a
   non-default → `400 unsupported_parameter`, unknown fields are proxied (open list).
   Response (the 4.1 vocabulary): non-stream → a Response object (`resp_*`; text
   blocks are merged into one message item with one output_text part, tool_use →
   function_call items `fc_*` with `call_id` = tool_use id and an arguments string;
   usage: input = input+cache_creation+cache_read with
   `input_tokens_details.cached_tokens`, reasoning_tokens from thinking_tokens; status
   completed/incomplete by stop_reason: max_tokens/context_window →
   `max_output_tokens`, refusal → `content_filter`); stream Messages SSE → Responses
   SSE (`response.created` → `response.in_progress` → per-block `output_item.added` /
   `content_part.added` / `output_text.delta|done` / `function_call_arguments.delta|done`
   / `output_item.done` → `response.completed` with the full object and usage; ping →
   a `: ping` comment frame; malformed known event/order, mid-stream `event: error`,
   and premature EOF → `error` → `response.failed`; unknown named events are ignored;
   output_index — a dense counter of its own, thinking blocks occupy no position).
   Errors — the same OpenAI envelope as the chat adapter with status preserved (402
   LowBalance too) and `Retry-After`. Temporary 4.1 limitations: `function_call`/
   `function_call_output` items in the input → `400 unsupported_parameter` (tool-call
   history replay — 4.2), `reasoning` items in the input are accepted and dropped
   (signatures are not exposed — decision 4), thinking blocks of the response are
   skipped without reasoning events, `store:true`/`previous_response_id`/
   `item_reference` → `400 documented_limitation`; **4.2** — replay of tool history
   in the input + reasoning summary events — **IMPLEMENTED**: input `function_call`
   items → Messages assistant `tool_use` blocks (`call_id` → `id`, the `arguments`
   JSON string is parsed into `input` — invalid JSON/non-object → `400
   invalid_request`, a missing/empty string — `{}`; missing/empty `call_id`/`name` →
   400), input `function_call_output` items → user `tool_result` blocks (`call_id` →
   `tool_use_id`; an `output` string → text content as-is, an array of text parts is
   merged with \n, non-text parts → 400); merging with neighboring message items is
   the shared same-role one; tool_use/tool_result pairing is not validated by the
   adapter (the Messages upstream honestly answers 400, as in the chat adapter 3.2).
   Thinking blocks of the response are translated into the Responses reasoning
   vocabulary: non-stream — a reasoning item
   `{"type":"reasoning","id":"rs_*","summary":[{"type":"summary_text","text":<block
   text>}]}` in the output in block order (each thinking block — a separate item; an
   empty thinking block spawns no item; the message item sits at the position of the
   first text block); stream — `response.output_item.added` (reasoning, summary []) →
   `response.reasoning_summary_part.added` (summary_index 0, an empty summary_text
   part) → `response.reasoning_summary_text.delta`* from thinking_delta (empty deltas
   and signature_delta are dropped) → `response.reasoning_summary_text.done` →
   `response.reasoning_summary_part.done` → `response.output_item.done`; output_index
   — a dense counter now including thinking blocks (redacted_thinking is skipped
   without a position — decision 4), and the reasoning item lands in the completed
   output; `output_tokens_details` from message_delta are proxied into usage
   (reasoning_tokens, as in non-stream). Signatures/encrypted_content are still not
   exposed (decision 4). Temporary limitations after 4.2:
   `store:true`/`previous_response_id`/`item_reference` → `400 documented_limitation`
   and `POST /v1/responses/input_tokens` openai-only (decision 5); `reasoning` items
   in the input are accepted and dropped. In the router the duplicated
   `namespace_lane` of the chat/responses dispatches was factored into a shared
   `pub(crate)` in `crates/router/src/catalog.rs`; **4.3** — the Gemini mirror
   (Responses→generateContent in the Gemini plane after the pattern of 3.3) —
   **IMPLEMENTED**: adapter `crates/forward/src/gemini/responses.rs`, a route `POST
   /v1/responses` in `ProviderMode::Gemini` (the router was not changed — dispatch of
   `google/*` and gemini aliases works since 4.1). The flow — the chat-adapter 3.3
   pattern: translation into a GenerateContentRequest → an internal request to
   `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse` → the shared
   `gemini_api()` unchanged → translation of the response ON THE OUTSIDE. The
   Responses side of the 4.1+4.2 vocabulary (item forms, SSE events, usage,
   status/incomplete_details) is identical to the Anthropic adapter and is pinned by
   the module's contract tests on the same tabular expectations. Request:
   `instructions` and system/developer items → `systemInstruction` (one text part per
   item, instructions first), `input` string/items → contents with same-role merging,
   `input_image` → inlineData by the shared translation (only data: URL — http(s) is
   not accepted by generateContent → `400 invalid_request`; `detail` != auto →
   `400 unsupported_parameter`), replay of function_call/function_call_output →
   functionCall/functionResponse parts (the `arguments` JSON string → `args`;
   functionResponse references the call by NAME — a call_id→name map over the
   history's function_call items, output without a pair → `400 invalid_request` — a
   difference from the Anthropic mirror, where pairing is not validated), `tools` →
   `[{"functionDeclarations": …}]` (a flat descriptor, `strict` stripped),
   `tool_choice` → `toolConfig.functionCallingConfig`, `max_output_tokens` →
   `generationConfig.maxOutputTokens` only with an explicit cap; omission leaves the
   field absent until the shared model-limit/balance admission; `reasoning.effort` →
   `generationConfig.thinkingConfig` (`thinkingLevel` proxied as-is — minimal is NOT
   clamped, a difference from the Anthropic mirror; `includeThoughts: true`),
   `text.format` json_schema → `responseMimeType: application/json` +
   `responseSchema` (the wrapper is stripped), json_object → `responseMimeType`
   (generateContent has it — a difference from Messages, where json_object → 400).
   The capability matrix — the same 9 rules as the Anthropic mirror plus
   `parallel_tool_calls` (generateContent has no disable_parallel_tool_use — only
   default true); UNKNOWN top-level fields → `400 unsupported_parameter` (a closed
   list, like the chat adapter 3.3 — the Code Assist wrapper would drop them
   silently).    Response: thought parts → reasoning items `rs_*` and reasoning_summary
   events of the 4.2 vocabulary (a part with only a thoughtSignature spawns no events
   — decision 4), functionCall → function_call items `fc_*` with synthesized call_id
   `callu_<name>[_N]` (there is no functionCall.id on the private wire — the chat
   adapter's scheme) and exactly one arguments delta (functionCall arrives whole),
   generated image-MIME `inlineData` parts → `output_image` items `img_*` with a
   data URL (non-stream and stream: item.added → item.done — billed media is
   delivered, not dropped);
   usage — input = `promptTokenCount`, output = `candidatesTokenCount`+
   `thoughtsTokenCount` (the same sum that metering bills),
   `cachedContentTokenCount` → `input_tokens_details.cached_tokens`,
   `thoughtsTokenCount` → `output_tokens_details.reasoning_tokens`;
   finishReason/blockReason → status via the shared `map_finish_reason`: MAX_TOKENS →
   incomplete `max_output_tokens`, SAFETY/RECITATION/BLOCKLIST/PROHIBITED_CONTENT/SPII
   → incomplete `content_filter`. Stream: data-only SSE → Responses SSE; normal
   termination of a Gemini stream — `finishReason`/`promptFeedback.blockReason` + a
   clean EOF (there is no message_stop on the wire): an open item is closed with done
   events and `response.completed` is emitted (a difference from the Anthropic
   mirror, where the full lifecycle up to `message_stop` is mandatory); a malformed
   provider frame, EOF without terminal evidence, a mid-stream error frame
   `{error:{code,message,status}}`, and a transport failure → `error` →
   `response.failed` (error.code — the google.rpc status). Errors — the same
   `convert_error_response` as the chat adapter (Google envelope → OpenAI envelope,
   native `400 API_KEY_INVALID` → `401 authentication_error`, 402 and `Retry-After`
   preserved). Tool declarations and the `text.format` schema pass through the shared
   Code Assist sanitizer; a replayed functionCall uses the same stateless
   context-engineering marker as Chat 3.3. Temporary limitations — as after 4.2:
   input reasoning items are dropped, `store:true`/`previous_response_id`/
   `item_reference` → `400 documented_limitation` (decision 5). The response's real
   `thoughtSignature` is neither stored nor publicly exposed per decision 4. The
   shared helpers (`chat_error`, `invalid_request`, `unsupported_parameter`,
   `convert_error_response`, `merge_or_push`,
   `gemini_image_part`/`translate_reasoning_effort`/`parse_tool_arguments` with the
   parameter name, `function_declaration`, `code_assist_schema`,
   `replayed_function_call_part`, `function_response_value`, `synthetic_call_id`,
   `map_finish_reason`, the limit constants) were factored into `pub(crate)` in
   `gemini/chat.rs` (after the pattern of the 4.1 factoring into `anthropic.rs`). No
   mock e2e smoke of the Gemini chain was added (the plane requires an encrypted
   OAuth pool, as in 3.3); the mock e2e coverage of the universal lane is the
   Anthropic chain in `tests/universal_chat_smoke.sh`; the production Gemini/OpenCode
   tool cycle is a separate live case of the harness matrix.
5. **Anthropic Skin for non-Claude models (3–5 weeks).** A Messages entry for
   GPT/Gemini: beta fields, tool streaming, thinking, error recovery, token counting —
   per decision 6 (a mirror of decisions 3–4, thinking without signatures). **5.1 —
   Anthropic Skin for `openai/*` models (Codex plane) — IMPLEMENTED.** In the router
   `POST /v1/messages` gained model-based dispatch (`crates/router/src/messages.rs`)
   under the same rules as the chat/responses dispatches of 3.1/4.1: only the request
   body is buffered (32 MiB), the `openai/` namespace prefix selects the Codex plane
   without querying the catalog (the shared `catalog::namespace_lane`; `anthropic/`
   and `google/` go to their own planes — the Gemini Messages skin is implemented in
   5.2 below), the rest — by alias through the cached catalog; the body is proxied
   unchanged, and dispatch errors come in the Anthropic envelope. A namespaced
   `anthropic/<id>` on the Anthropic plane is stripped by the plane's admission
   before reserve and upstream (`strip_own_namespace` in
   `crates/forward/src/proxy.rs`, a mirror of the chat adapter 3.x strip): before this
   fix the prefix reached the upstream byte-identical and it answered 404 (production
   probe 2026-08-01). `POST /v1/messages/count_tokens` uses the same model-based
   dispatch: native Anthropic lane, local reserve-grade Codex counting, or the Gemini
   endpoint from 5.2. On the Codex plane (`crates/forward/src/codex/skin.rs`, routes
   `/v1/messages` and `/v1/messages/count_tokens` in `ProviderMode::OpenAi`) a
   Messages request is translated into Responses JSON and goes through the same turn
   pipeline as the chat adapter (admission, affinity, reserve, run, settle by
   authoritative usage): stripping the `openai/` prefix, `speed:"fast"` and the
   compatible `service_tier:"fast"|"priority"` → canonical Responses
   `service_tier:"priority"` (other/absent values → Standard), top-level `system` (a
   string or text blocks, merged with \n\n) → `instructions`, user text/image blocks
   → `input_text`/`input_image` (the shared `canonical_image_part`), assistant text →
   `output_text`, tool-history replay — a mirror of 4.2 (`tool_use` → `function_call`
   with a `call_id` and an arguments string, `tool_result` → `function_call_output`;
   pairing is not validated), thinking/redacted_thinking of input blocks are dropped
   (decision 6), `tools[]` → function tools (`input_schema` → `parameters`; server
   tools → 400), `tool_choice` auto/any/none/tool → default/required/none/named
   (+`disable_parallel_tool_use` → `parallel_tool_calls:false`), `thinking` →
   `reasoning.effort` (lossy: disabled/adaptive → model default; enabled budget <4096
   → low, <16384 → medium, otherwise high; <1024 → 400), `stop_sequences` and
   `max_tokens` are honestly processed on the delivered text by the shared
   `StopFilter` and an output budget of ~4 chars/token (as in chat.rs — the transport
   cannot clip generation upstream). Capability matrix: stateful/unknown
   `cache_control` anywhere (system, content blocks, tools), stateful/unknown
   `context_management`, `mcp_servers`, `container` → `400 invalid_request_error`
   with the parameter name. The exact Claude Code `cache_control:{type:"ephemeral"}`
   is accepted and stripped: Codex prompt caching is automatic; any extension of the
   marker remains fail-closed. The bounded no-op that Claude Code 2.1.220 sends by
   default (`context_management.edits` empty or containing exactly
   `{type:"clear_thinking_20251015",keep:"all"}`) is accepted and stripped: the
   stateless adapter already drops input thinking blocks; additional fields, edits,
   and values remain fail-closed. Native Messages `output_config.effort`
   (low/medium/high) → Responses `reasoning.effort`, and the bounded GA
   `output_config.format` json_schema → Responses `text.format` with the same schema;
   unknown keys and unrepresentable shapes → 400. This supports both the structured
   title request and the main adaptive turn of current Claude Code; `metadata`
   (including `user_id`), sampling controls, and unknown fields are accepted and
   ignored (the same leniency as chat.rs — Claude Code sends `metadata.user_id` in
   every request). The response — a mirror of the 4.1+4.2 vocabulary: message items →
   a text block at the position of the first message item, `function_call` →
   `tool_use` (arguments are parsed into `input`, invalid JSON → `{}`), `reasoning` →
   thinking blocks WITHOUT a signature (summary parts merged with \n\n), usage →
   Messages usage (cache write/read → `cache_creation_input_tokens`/
   `cache_read_input_tokens` when >0, reasoning →
   `output_tokens_details.thinking_tokens`, effective tier → `service_tier`),
   stop_reason: function_call in the output → `tool_use`, an output-budget cut →
   `max_tokens`, a matched stop_sequence → `stop_sequence`, otherwise `end_turn`.
   SSE: `message_start` with zero usage (authoritative usage exists only at the end
   of the turn — a documented limitation) → per-block
   `content_block_start`/`content_block_delta` (`text_delta`, `thinking_delta`,
   `input_json_delta`)/`content_block_stop` (dense indexes, a new block type closes
   the previous one) → `message_delta` (stop reason + usage) → `message_stop`, but
   only after the source `finishReason`/`blockReason` + EOF; heartbeat — `event:
   ping`, malformed/premature EOF and a mid-stream failure — `event: error`; a client
   disconnect does not kill the turn — it runs to authoritative usage for settlement
   (as in chat.rs). All endpoint errors (adapter validation, the shared parser,
   admission, billing) are rebuilt into the Anthropic envelope with status and
   `Retry-After` preserved (503 → 529 `overloaded_error`, 402 is preserved — Claude
   Code recovers based on the error text). `POST /v1/messages/count_tokens` on the
   plane — the same parse + `parse_responses_request`/`prepare_turn` → a reserve-grade
   `input_tokens` estimate without network (`max_tokens` is optional there, as in the
   official endpoint). 5.1 limitations: the body limit is 8 MiB (the plane's shared
   `OPENAI_BODY_LIMIT`, not 32); there is no end-to-end e2e smoke of the Codex plane
   (the harness cannot do encrypted OAuth profiles — coverage by unit/contract tests,
   as in 3.3/4.3). **5.2 — Anthropic Skin for `google/*` models (Gemini plane) —
   IMPLEMENTED.** The Gemini mirror of 5.1 (`crates/forward/src/gemini/skin.rs`,
   routes `/v1/messages` and `/v1/messages/count_tokens` in `ProviderMode::Gemini`;
   the router was not changed — dispatch of `google/*` and gemini aliases works since
   5.1): the Messages side of the vocabulary is identical to 5.1
   (system/messages/tools/tool_choice/thinking/capability matrix, Messages SSE, the
   Anthropic error envelope — contract tests of both modules on equivalent input),
   request translation and response parsing — by the rules of the plane's
   chat/responses adapters (3.3/4.3), with shared helpers reused from `gemini/chat.rs`
   without changing its logic. Request: stripping the `google/` prefix BEFORE
   admission, top-level `system` → `systemInstruction` (merged with \n\n, a
   non-default `cache_control` → 400), messages → contents via the shared
   `merge_or_push` (assistant → role model; `tool_use` → functionCall with `args` as
   an OBJECT — not a JSON string, a difference from the Codex side; `tool_result` →
   functionResponse, pairing via the id→name map is validated — the 3.3/4.3 pattern),
   image: only base64 → inlineData (url source → 400), input thinking is dropped
   (decision 6); `disable_parallel_tool_use: true` → 400 (generateContent has no
   analog); sampling (temperature/top_p/top_k) and `stop_sequences` are proxied into
   generationConfig (it supports them natively — a plane-level difference from 5.1;
   stop_reason `stop_sequence` is indistinguishable → `end_turn`); the capability
   matrix — the same 4 rules of 5.1 PLUS a closed list of top-level fields (unknown →
   400, as in chat.rs). Response: text parts → one text block, thought parts →
   thinking blocks WITHOUT a signature, functionCall → `tool_use` with a synthesized
   `toolu_<name>[_N]`, usage — input=`promptTokenCount`, output=candidates+thoughts
   (thoughts → `output_tokens_details.thinking_tokens`, cached →
   `cache_read_input_tokens`). The handlers go through the shared `gemini_api()` via
   an internal Request to `generateContent|streamGenerateContent?alt=sse|:countTokens`
   — admission, reserve, affinity, rotation, Code Assist wrapper, and usage settlement
   without a single change; `count_tokens` — native `:countTokens` (quota-free, no
   reserve), `max_tokens` is optional there. Tool schemas pass through the shared
   sanitizer, and a replayed tool_use gets the stateless context-engineering marker;
   actual provider signatures of the response remain hidden per decision 4. The body
   limit — the plane's shared one; there is no mock e2e smoke of the Gemini plane (as
   in 3.3/4.3 — coverage by the module's unit/contract tests); the production OpenCode
   tool cycle is pinned by the live harness matrix. Router dispatch of
   `/v1/messages/count_tokens` is covered by integration mock tests of the namespace
   and alias paths of all three planes.
6. **OpenRouter-grade routing (2–4 weeks).** Provider preferences, explicit fallback
   lists, attempt fencing (execution group / single billable winner, see "Fallback
   semantics"), per-account policy, telemetry, presets. Per decision 7 the first
   package of the stage is a detailed design on the live telemetry of stages 3–5 — it
   is fixed in `docs/engine/ROUTING_FENCING.md` (fact base, the
   `execution_state=not_started` contract, group/attempt identity, phasing 6.1–6.4).
   Phase 6.1 is implemented (2026-08-01): the planes emit `x-apitoken-execution-state:
   not_started` on non-2xx failures before the started boundary with a refund/cancel
   guarantee of the reserve; the router strips the header from all transit responses.
   The Gemini Messages skin and the universal Chat/Responses adapters of both
   translating planes preserve the signal on pre-delivery non-2xx and strip it from
   rebuilt errors after 2xx, when a charge is possible (§3.2 there). Phase 6.2 is
   implemented (2026-08-02): the shared router engine accepts the default-off
   `models`, preflight-validates the chain, and does a serial retry only on the exact
   signal or ConnectionRefused; timeout/unsigned 5xx/client 4xx fail closed, and the
   internal header is never visible to the client. Phase 6.3 is implemented
   (2026-08-02): the trusted group/attempt identity travels router→plane→reservation,
   and SQLite/PostgreSQL settle selects exactly one billable winner and fully refunds
   the loser hold; `ExecutionGroupDoubleWinner` gates any such incident. The phase
   6.4 contract was fixed 2026-08-02: the identical authenticated policy preflight on
   the planes is implemented producer-first, and the router consumer 6.4b added strict
   `provider` preferences, version-controlled presets/ranks, and fail-closed policy
   filtering before attempt 1. Router/plane metrics, Prometheus rules/runbooks,
   exact-delta mock-load, and the credential-safe live runner are implemented
   default-off; after their rollout come live canary of the exact deployed binary and
   a separate enabling of the production flag. The full scheme, fail-closed
   mixed-version semantics, and rollout order — `docs/engine/ROUTING_FENCING.md`
   §5.1–5.3. Separately — Stage 3 HA: a second host, router replicas, HA PostgreSQL
   (see the limitations in `docs/engine/STAGE2_POSTGRES_AUTHORITY.md`: loss of the
   single host is not covered yet — that is Stage 3, not a router blocker).

A useful unified native endpoint — stages 1a–1b. Production-grade multiprotocol
parity — roughly 8–14 engineer-weeks sequentially. The main difficulty is not failure
isolation (it already exists) but correct translation of tools/reasoning/streaming
and exactly-once billing.

## Open decisions

- ~~Public domain name~~ — decided at stage 1a: `router.apitoken.sale` (a new
  separate hostname; `api.apitoken.sale` is not reused and does not change behavior).
- ~~Partial-catalog policy of `/v1/models` when a plane is down~~ — decided at stage
  1b: 30 s TTL cache + TTL-less last-good; a downed plane is omitted from the
  listing, degradation is marked with the `x-apitoken-catalog-degraded` header with a
  list of namespaces; an empty plane catalog counts as a failure and is not cached; a
  401/403 from any plane → a unified 401 `invalid_api_key`; all planes unavailable
  without cache → 503 `catalog_unavailable`.
- ~~Gemini product scope~~ — decided 2026-08-02: Gemini is part of the target main
  product and pricing release. Service gets all runtime-capable Gemini models;
  OpenKeys includes Gemini only through an explicit OpenKeys catalog generation and
  always 1:1. Producer schema/catalog expansion is delivered before the consumer and
  before the single Stage 9 cutover.
