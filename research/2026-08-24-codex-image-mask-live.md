# Live: GPT Image 2 mask on ChatGPT Codex OAuth (2026-08-24)

This is a probe record, not a product instruction. It answers whether **alpha-mask
inpainting** exists on the native ChatGPT Codex wire (`chatgpt.com/backend-api/codex`),
as opposed to official `api.openai.com`.

Session: local Codex CLI **0.149.1**, `auth_mode=chatgpt`, plan **pro**, OAuth access
token still valid. Requests used `originator: codex_cli_rs`, `version: 0.149.1`,
`User-Agent: codex_cli_rs/0.149.1 (macOS; arm64) codex_cli_rs`, `ChatGPT-Account-ID`.
The local CLI `openai_base_url` points at a loopback router; this probe **did not**
use that proxy. It called ChatGPT directly. Tokens and account identifiers are not
in this file. Raw bodies stayed in `/tmp/codex-mask-probe/` and are not committed.

Fixtures: 1024×1024 PNG, solid blue `(32,96,208)`. Mask PNG, same size, **left half
alpha 0** (inpaint), **right half alpha 255** (keep). Quality `low`, size `auto`.
ChatGPT returned `1254x1254` `opaque`/`low` PNGs, matching earlier GPT Image 2 canaries.

## Verdict

| Surface | Mask inpainting? | Evidence |
|---|---|---|
| Native `POST /codex/images/edits` JSON field `mask` | **No. Extra key is ignored.** | 200, same 1024 image-input tokens with or without `mask`; whole-red prompt paints **both** halves red. |
| Native `images/edits` with mask PNG as a second `images[]` entry | **No. It is a second reference, not an alpha mask.** | 2048 image-input tokens; output still uniform red. |
| Public OpenAI Images `multipart mask` on this wire | **No typed field.** Official Codex `ImageEditRequest` has `images`, `prompt`, `background`, `model`, `n`, `quality`, `size` only (0.149 binary + GitHub `master` `codex-api/src/images.rs`). | CLI `--mask` is documented as **API-key fallback** (`OPENAI_API_KEY` → `POST /v1/images/edits`), not built-in `image_gen`. |
| Responses hosted `image_generation` + `input_image_mask.image_url` (data URL) | **Yes, live.** | Prompt said fill the **entire** canvas red. Output is left red / right original blue. `action: "edit"`. `revised_prompt` names the supplied mask. Image-input tokens stay 1024 (mask not billed as a second image). |
| Same tool + `input_image_mask.file_id` | **No usable Files API.** | Stream starts `image_generation_call.generating` then `error` / `response.failed` (`server_error`). |
| Codex 0.149 built-in `image_gen` tool args | **No mask parameter.** | `ImagegenArgs` is `prompt`, `referenced_image_paths`, `num_last_images_to_include` only. |

So: **mask is possible on Codex OAuth, but only as Responses `image_generation.input_image_mask` with an `image_url` data URL.** It is not possible as Images API `mask`, not as an extra JSON field on native `/images/edits`, and not via `file_id`.

Our public `POST /v1/images/edits` reject of `mask` remains correct. Forwarding that field would 200 and **not** inpaint.

## 1. Official client vs official API docs

OpenAI Images/Responses docs describe `mask` / `input_image_mask` (often `file_id` from the
Files API). That is `api.openai.com`.

Codex 0.149 skills (`~/.codex/skills/.system/imagegen/`):

- Built-in `image_gen` does **not** take mask / quality / `input_fidelity` as tool args.
- `--mask` exists only on `scripts/image_gen.py` fallback, which needs `OPENAI_API_KEY` and
  calls `client.images.edit(..., mask=...)`.
- Quote: “Do not assume they are normal arguments on the built-in `image_gen` tool.”

This session’s `auth.json` has no API key (`OPENAI_API_KEY` is null). The fallback CLI
cannot run on this ChatGPT login.

## 2. Native `/images/edits` — extra `mask` is ignored

Discriminating prompt (probe 2): *“Fill the entire canvas with uniform bright red. Do not keep any blue.”*

If an alpha mask were applied, the **right** half would stay blue.

| Call | HTTP | Image-input tokens | Left RGB | Right RGB | Sample red/blue |
|---|---|---|---|---|---|
| No `mask` | 200 | 1024 | 249.6, 2.1, 3.1 | 249.5, 2.1, 3.1 | 24649 / 0 |
| `mask` = PNG data URL string | 200 | 1024 | 250.9, 2.1, 2.6 | 250.8, 2.1, 2.6 | 24649 / 0 |
| Mask PNG as second `images[]` | 200 | **2048** | 250.9, 2.2, 3.1 | 250.7, 2.2, 3.0 | 24649 / 0 |

`mask` as a sibling JSON key does not add tokens and does not protect the right half.
A second `images[]` entry **does** add a second 1024-token reference and still does not
inpaint. That is whole-image edit from references + prompt.

Probe 1 used “paint the left half red, keep the right blue.” Both the masked and the
unmasked calls produced a red/blue split. That prompt cannot prove a mask channel; the
model followed the text. Probe 2 is the one that counts.

## 3. Responses `image_generation` + `input_image_mask.image_url` — live inpaint

Same whole-red prompt. Tool:

```json
{
  "type": "image_generation",
  "quality": "low",
  "size": "auto",
  "background": "opaque",
  "input_image_mask": { "image_url": "<png data URL>" }
}
```

Input also carried the blue PNG as `input_image`. Forced `tool_choice.type=image_generation`.
HTTP 200, SSE completed.

Observed:

- `image_generation_call` with `action: "edit"`, `quality: "low"`, `size: "1254x1254"`,
  `background: "opaque"`, `output_format: "png"`.
- `revised_prompt` begins: *“Edit the provided image using the supplied mask. Change only
  the masked/allowed left half to a perfectly uniform, solid bright red…”*
- Pixels: left `(248.6, 1.0, 1.0)` red, right `(33.8, 97.6, 208.5)` original blue.
  Sample counts 12403 red / 12246 blue.
- `tool_usage.image_gen`: 1024 image-input tokens, 229 image-output tokens. The mask was
  not billed as a second image.
- `response.completed.tools[0]` echo is `type/background/model=gpt-image-2-codex/...`
  **without** `input_image_mask`. The backend consumed the field; it does not round-trip
  it in the completed tool list.

A bogus `input_image_mask.file_id` (`file-aaa…`) started `image_generation_call.generating`
then failed with generic `server_error` / `response.failed`. No Files API on this plane.

## 4. What this means for our gateway

Current production (`POST /v1/images/edits`) rejects multipart `mask`. Keep that. Native
`/images/edits` would ignore the field and still 200.

Current production Responses hosted `image_generation` **clones** the tool object
(`hosted_tool_descriptor`). Extra keys including `input_image_mask` can reach ChatGPT.
This probe did **not** go through `openai.api.apitoken.sale`; it used the same native
backend the pool calls. A customer who already sends `input_image_mask.image_url` on
`/v1/responses` can get inpaint after the hosted-tool forward (SHA `be6c88b1` and later).
`file_id` will not.

Product follow-up (this repository): the gateway now parses `input_image_mask.image_url`,
forwards the PNG data URL, and fail-closes `file_id`. Images HTTP `mask` stays rejected.
Client contract: `docs/engine/CODEX_PROVIDER.md` § Region inpaint and the public docs
portal (`/docs` API reference + GPT Image 2 / image-editing learn guides).

Codex CLI’s own `image_gen` tool still cannot express a mask. Clients must send the
Responses tool field themselves.

## 5. Spend / quota

Paid ChatGPT image turns on this login (low quality, auto size): two edits in probe 1,
three edits in probe 2, two Responses image generations (one success, one `file_id`
failure). No OpenAI API-key dollars. No production customer key.

## 6. Do not confuse with

- Reference-image edit without a mask (already public on `/v1/images/edits`).
- Prompt-only “edit the left half” (model can split without a mask).
- Official `api.openai.com` Images `mask` (different auth, Files API, multipart).
