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

## Gaps (the client asked for these; we do not deliver them)

### 1. Hosted `web_search` inside Responses

The client needs a server-executed web search tool on `POST /v1/responses`.

Our gateway never forwards hosted `web_search`. The descriptor is accepted and
dropped so stock Codex CLI configs do not 400. The model receives no search tool.
A nested `web_search` inside a namespace fails closed.

A client that sends `tools: [{ "type": "web_search" }]` gets HTTP 200 and no
search. That is not OpenAI hosted search.

Code: `crates/forward/src/codex/api.rs` (`hosted_web_search_is_accepted_and_never_forwarded`).

### 2. Hosted `code_interpreter` inside Responses

No `code_interpreter` parser, container, file mount, or Python execution exists
in the repository. The type is an unknown hosted descriptor: accepted and dropped,
same as `web_search`. The model never runs code on our side.

OpenAI `include: ["code_interpreter_call.outputs"]` is ignored (unknown `include`
values are ignored; only `reasoning.encrypted_content` is honoured).

### 3. Hosted `tool_search` (OpenAI platform form)

The client named hosted tool search inside Responses.

What we have is Codex CLI **client-executed** `tool_search` only
(`execution` must be `"client"`). The gateway rewrites it to a local function
`__codex_client_tool_search` and returns the call to the client. The client must
search its own tool list and send `tool_search_output`.

OpenAI hosted `tool_search` (server-side deferred discovery, execution other than
`"client"`) is rejected: `tool_search.execution must be "client"`.

This is not the hosted tool the client described.

### 4. Strict JSON Schema (`strict: true`)

The client needs Responses structured outputs with `strict` JSON Schema.

On the Codex plane `strict: true` is silently degraded to `strict: false`. Extra
tool fields including `additionalProperties` flags are ignored. Upstream
`text.format` for `json_schema` is always sent with `"strict": false`.

The request succeeds. Schema is not enforced as OpenAI Structured Outputs strict
mode.

Code: `parse_top_level_function`, `parse_additional_function`,
`upstream_tool` in `crates/forward/src/codex/{api,runner}.rs`;
test `strict_function_tools_are_silently_downgraded`.

### 5. Prompt cache 24-hour retention

The client needs `prompt_cache_key` **and** 24-hour hold.

We accept `prompt_cache_key` (echoed in the public response; upstream key is
bounded to 64 bytes / hashed). We do **not** parse or honour:

- `prompt_cache_retention: "24h"` (OpenAI older control; ignored as an unknown
  top-level field on native Codex Responses)
- `prompt_cache_options.ttl` (OpenAI GPT-5.6+ minimum lifetime; absent)

Cache lifetime is whatever the ChatGPT Codex backend keeps (typical OpenAI
in-memory window is minutes, not a guaranteed 24h). We cannot promise or meter a
24h retention policy.

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

These requests look successful and do not do what OpenAI does:

| Client sends | What happens here |
|---|---|
| `tools: [{type:"web_search"}]` | Descriptor dropped; no search |
| `tools: [{type:"code_interpreter"}]` | Descriptor dropped; no Python |
| `tools[].strict: true` or `text.format.strict: true` | Forced `strict: false` |
| `prompt_cache_retention: "24h"` | Ignored; no 24h hold |
| `parallel_tool_calls: false` | Ignored; transport always runs parallel |
| `tool_choice: "required"` / named tool | Degraded to `"auto"` |

`tool_search` without `execution: "client"` is the exception: it 400s.

## Architectural cause

Hosted tools, 24h KV cache policy, embeddings, transcription, and mask inpainting
are OpenAI **platform** services. Our OpenAI plane forwards to
`chatgpt.com/backend-api/codex` with a sealed ChatGPT OAuth profile. That backend
does not expose those services in a form we can meter. The gateway therefore
drops or rejects them instead of proxying an unbilled official API.

Closing these gaps is not a router alias. It needs either a real OpenAI API-key
plane or a separately proven hosted-tool/media producer with its own tariff and
live evidence.
