# Gap analysis: OpenAI Responses client request (2026-08-24)

Snapshot of a sales qualification against `origin/master`
`16d3f4db7a27753653c57e87ac02d00a12c3b631`. This is not a product instruction.
Sources: `docs/engine/CODEX_PROVIDER.md`, `docs/engine/UNIFIED_ROUTER.md`,
`crates/forward/src/codex/{api,runner,image_api}.rs`, `crates/metering/src/codex.rs`,
`crates/router/src/main.rs`.

The client wants a drop-in for official OpenAI Responses API plus neighbouring
OpenAI surfaces. Our OpenAI plane is a ChatGPT Codex OAuth subscription pool, not
`api.openai.com`. Hosted platform tools, 24h prompt-cache retention, embeddings,
Whisper, and mask inpainting do not exist on that wire.

## Status after the RN Codex-plane fixes

Closed slices (parser/wire only; no live ChatGPT probe of search or constrained
decoding):

- **hosted execution (live Pro 2026-08-24).** `web_search` and `image_generation`
  execute on ChatGPT Codex `/responses` (`web_search_call` / `image_generation_call`
  items observed). `code_interpreter` is upstream `Unsupported tool type` → gateway
  `400 documented_limitation`. Search settles at `$0.01`/call. `file_search` /
  `computer*` / `mcp` stay 400. `prompt_cache_retention` stays 400.
- **strict-json-schema.** Function-tool `strict` and `text.format.strict` are
  kept on the parsed request and sent upstream. They are not rewritten to false.
  Chat still copies `strict`. Extra `additionalProperties` flags on tools stay
  ignored.
- **hosted-tool-search.** Hosted `tool_search` (`execution` omitted/`server`/
  `hosted`) is forwarded as `type:tool_search`. Client `execution:"client"` still
  rewrites to `__codex_client_tool_search`. The gateway does not search tools
  itself.

Still not delivered (bucket B/C): embeddings, transcription, mask inpainting,
code_interpreter execution, 24h KV cache, metered hosted web_search, an OpenAI
API-key plane.

## Remaining gaps

### 1. Hosted `web_search` — forwarded and billed; live ChatGPT proof pending

The gateway now forwards `web_search` and bills `web_search_call` at $0.01.
Live proof on a production subscription is the remaining gate.

### 2. Hosted `code_interpreter` — cannot execute on this wire

Live Pro probe 2026-08-24: ChatGPT Codex `/responses` returned
`400 Unsupported tool type: code_interpreter`. The gateway fails closed with
`400 documented_limitation`. A local Python container is a different product.

### 3. Hosted `tool_search` — forwarded, not gateway-executed

Hosted descriptors now reach the Codex backend as `type:tool_search`. This
gateway does not search the request's tool list itself. Whether ChatGPT executes
the hosted form is unproven live.

### 4. Strict JSON Schema — forwarded, not live-proven

`strict: true` is no longer rewritten to false. Whether the ChatGPT backend
actually constrains output is unproven. Extra `additionalProperties` flags on
tools are still ignored.

### 5. Prompt cache 24-hour retention

The client needs `prompt_cache_key` **and** 24-hour hold.

We accept `prompt_cache_key` (echoed in the public response; upstream key is
bounded to 64 bytes / hashed). Present non-null `prompt_cache_retention` and
`prompt_cache_options` now fail closed with `400 documented_limitation` on that
field: this plane cannot honour 24-hour KV retention, and a silent 200 would
claim it did. Null/omitted values stay accepted.

Cache lifetime is still whatever the ChatGPT Codex backend keeps (typical OpenAI
in-memory window is minutes, not a guaranteed 24h). A 400 is honest. It is not a
24h hold.

Covered parts of the same ask (not gaps): `usage.input_tokens_details.cached_tokens`
is copied from authoritative upstream usage; cached input is billed at the official
~0.1× fresh-input tariff (`docs/commerce/PRICING.md`, `crates/metering/src/codex.rs`).

### 6. Embeddings

No `POST /v1/embeddings` route on the unified router or the OpenAI plane.
Unknown paths return an OpenAI-shaped 404 (`crates/router/src/main.rs` fallback).
Observability lists embeddings as an excluded surface. No Whisper/embedding
producer exists.

### 7. Audio transcription

No `POST /v1/audio/transcriptions` (Whisper / gpt-4o-transcribe).

The only audio surface in this repository is Suno **music generation**
(`POST /v1/audio/generations`), default-off, not speech-to-text.

### 8. Image edits with a mask

`POST /v1/images/edits` exists for GPT Image 2, but the public contract rejects
the multipart field `mask` (`Field mask is not supported by the verified image
pool contract.`). Native Codex image wire has no mask. Transparency, exact size,
medium/high quality, JPEG/WebP, streaming partial images, and Responses hosted
`image_generation` are also rejected or unproven.

The client marked mask inpainting as mandatory. Reference-image edit without a
mask is not that feature.

### Adjacent image limits (not named as mandatory, but easy to overpromise)

Public image generation **does** exist (`POST /v1/images/generations`), one
non-streaming `opaque` / `low` / `auto` PNG, `n=1`, `b64_json` only. It is not
the full OpenAI Images/Responses image tool. Image models sent to
`/v1/responses` get `400` that names the image routes.

## Silent-success traps (worse than a 400)

| Client sends | After RN fixes |
|---|---|
| `tools: [{type:"web_search"}]` (API shape) | `400 documented_limitation` |
| Codex CLI `web_search` (`external_web_access` / `search_content_types`) | Still dropped, not forwarded (CLI compat) |
| `tools: [{type:"code_interpreter"}]` (and file_search/computer/mcp/image_generation) | `400 documented_limitation` |
| `tools[].strict: true` or `text.format.strict: true` | Forwarded upstream |
| `prompt_cache_retention: "24h"` / `prompt_cache_options` | `400 documented_limitation` |
| `parallel_tool_calls: false` | Still ignored; transport always runs parallel |
| `tool_choice: "required"` / named tool | Still degraded to `"auto"` |
| Hosted `tool_search` | Forwarded as `type:tool_search` |

## Architectural cause

Hosted tools, 24h KV cache policy, embeddings, transcription, and mask inpainting
are OpenAI **platform** services. Our OpenAI plane forwards to
`chatgpt.com/backend-api/codex` with a sealed ChatGPT OAuth profile. That backend
does not expose those services in a form we can meter. The gateway therefore
drops or rejects them instead of proxying an unbilled official API.

Closing these gaps is not a router alias. It needs either a real OpenAI API-key
plane or a separately proven hosted-tool/media producer with its own tariff and
live evidence.

## Feasibility (can close vs cannot)

Three buckets. "Closeable" still requires the normal GA gate: live proof,
authoritative usage, tariff, and a watchdog-GREEN SHA. A mock 200 is not enough.

### A. Closeable on the current Codex ChatGPT OAuth plane

Parser/wire rows below that the RN closed are marked **implemented**. Remaining
rows still need a live probe, a tariff, or more product work. A mock 200 is not
enough.

| Gap | Status | Remaining blocker |
|---|---|---|
| Strict JSON Schema | **Implemented** on this branch: `tools[].strict` and `text.format.strict` are forwarded, not rewritten to false. | Probe the Codex `/responses` backend with `strict: true`. If it constrains output, we can claim Structured Outputs. If it ignores the flag, we still cannot. If it 400s, this row moves to bucket C. |
| Hosted `web_search` | **Partial.** API `web_search` is `400 documented_limitation`. Codex CLI stock descriptor is still dropped, never forwarded. Settlement still hardcodes `web_search_requests: 0`. | Forward on an explicit API request, capture search usage, add a Codex search tariff. Keep dropping the stock Codex CLI cached descriptor, or every CLI turn starts a paid search. |
| Hosted `tool_search` | **Implemented** on this branch for the descriptor: omitted/`server`/`hosted` is forwarded as `type:tool_search`. Client `execution:"client"` still rewrites. The gateway does not search the tool list itself. | Live proof that ChatGPT executes the hosted form. Gateway-side search over the request's own tools is a different product and is **not** OpenAI catalog/MCP search. |
| Responses `image_generation` tool wrapping `/v1/images/*` | Type now fails closed and names the image routes. No Responses hosted-tool adapter. | Still the narrow GPT Image 2 contract (one `opaque/low/auto` PNG). Not mask inpainting. |
| Fail-closed errors instead of silent drop | **Implemented** on this branch for known unexecutable hosted types and 24h cache fields. Unknown future types stay dropped. | Does not add capability. |

Affinity already pins a conversation to one home via `prompt_cache_key`. That
raises short-cache hit rate. It is not 24-hour OpenAI KV retention.

### B. Closeable only as a new producer (different wire and money)

The Codex plane has no embeddings, Whisper, Python containers, or mask field.
A new producer can expose the OpenAI-shaped route. It is a different product:
we pay a vendor API (or run a sandbox) and resell. That is not ChatGPT quota
arbitrage. It still needs `docs/engine/PROVIDER_ONBOARDING.md` GA.

| Gap | How to close | What the client does **not** get |
|---|---|---|
| Embeddings | New `POST /v1/embeddings` against OpenAI API keys, Gemini embedding keys, or another embedding vendor. | Not served from ChatGPT Plus/Pro/Business OAuth. Price follows the vendor card plus our multiplier. |
| Audio transcription | New `POST /v1/audio/transcriptions` against Whisper / gpt-4o-transcribe keys or another STT vendor. Gemini audio-in-`generateContent` is a workaround, not the Whisper contract. | Not a Codex route. Latency, language, and diarization will follow the chosen vendor. |
| Image edits **with a mask** | OpenAI **API-key** Images API has `mask`. Native Codex image JSON has no mask field (`research/GPT_IMAGE_2_EVIDENCE.md`). | Not the current GPT Image 2 OAuth pool. Gemini reference-image edit is not an OpenAI alpha mask. A local composite of mask+image is not the model. |
| Hosted `code_interpreter` | (1) OpenAI API-key Responses with their containers, or (2) our own sandbox that executes Python and emits `code_interpreter_call` events. | ChatGPT Codex has no container API on `/responses`. Mapping to the client's local Lark `exec` is not hosted. A sandbox is a new security domain. |
| 24-hour prompt cache as OpenAI documents it | Only an OpenAI **API-key** plane honours `prompt_cache_retention: "24h"` / `prompt_cache_options.ttl`. | We cannot set GPU KV lifetime on ChatGPT. Echoing the field and pinning affinity is not a 24h hold. |
| Full official hosted-tool identity | A dedicated OpenAI API-key plane (web_search, code_interpreter, file_search, 24h cache, embeddings, Whisper, mask) at OpenAI list prices plus markup. | This is a reseller, not the subscription pool. Cost advantage of ChatGPT OAuth disappears for those legs. |

### C. Impossible on the current Codex wire (do not promise)

These are absent from the ChatGPT Codex backend we actually call
(`chatgpt.com/backend-api/codex`). No router alias creates them.

| Gap | Why it is impossible here |
|---|---|
| `prompt_cache_retention: "24h"` on GPT via ChatGPT OAuth | The backend does not take this field. Cache TTL is theirs. We do not hold their KV tensors. |
| Mask inpainting on the current GPT Image 2 OAuth pool | Official Codex image request types have no `mask`. The public route rejects the field. Live canary never proved masks. Sending a field the backend does not own will not inpaint. |
| OpenAI Python **containers** on Codex OAuth | No container id, file mount, or `code_interpreter_call.outputs` producer on this wire. |
| Embeddings or Whisper from a ChatGPT subscription profile | Those endpoints are not on the Codex backend. A 404 is correct. |
| Byte-for-byte OpenAI platform hosted tools (their search index, their container image, their 24h cache policy) while still spending only ChatGPT subscription quota | Two different products. The subscription pool cannot impersonate `api.openai.com` platform services. |

### Practical split for this client

- **Say yes, with work:** native Responses, SSE, reasoning, `store: false` + encrypted
  reasoning, parallel tools, `prompt_cache_key`, honest `cached_tokens` at ~0.1×,
  narrow image generation. Optional: stop silent drops; probe `strict`; probe
  Codex `web_search` metering; gateway-side tool_search over their tool list.
- **Say "paid add-on, new plane":** embeddings, transcription, mask edits,
  hosted code interpreter, 24h cache. Each is a vendor API or a sandbox, not a
  flag on Codex.
- **Say no:** drop-in of official OpenAI hosted tools plus 24h KV cache plus
  mask, all billed as ChatGPT subscription usage.

Recommended first live probes if we pursue the deal: (1) `strict: true` on one
Codex home, (2) explicit `web_search` with usage capture. Those two decide
whether bucket A is real or moves to C. Do not publish either without
authoritative usage and a tariff.
