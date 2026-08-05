# Gemini image interactions capability audit — 2026-08-05

## Snapshot and evidence boundary

This append-only historical audit records the repository at
`558d4b34896792cfaed5760852f9001feb0d0443`. Repository citations are line locations at that
exact SHA; later edits can move or invalidate them. Google's mutable official pages were reviewed
on **2026-08-05**. No fresh live, paid, or provider calls were made for this audit. Conclusions
about current provider behavior therefore stop at dated official documentation, repository code,
checked-in tests, and earlier evidence already present in the repository; they are not a new live
acceptance result.

Primary official sources reviewed:

- [Image generation guide, including multi-turn image editing](https://ai.google.dev/gemini-api/docs/image-generation#multi-turn-image-editing)
- [Interactions API overview](https://ai.google.dev/gemini-api/docs/interactions-overview)
- [Interactions REST reference](https://ai.google.dev/api/interactions-api)
- [`gemini-3.1-flash-image` model page](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-image)
- [Google GenAI SDK libraries](https://ai.google.dev/gemini-api/docs/libraries)
- [SynthID safeguard](https://ai.google.dev/responsible/docs/safeguards/synthid)

The official pages are not internally immutable. On the review date, they differed on details such
as the small-resolution spelling, some image token figures, output MIME examples, whether thinking
can be fully disabled, custom Interactions safety settings, and `v1beta` versus `v1beta2` in a
migration example. This report treats the image guide's canonical `POST /v1beta/interactions` flow
as the relevant current contract and does not convert conflicting details into gateway promises.

## Executive finding

Google's current image article is **Interactions-first**. It recommends the generally available
Interactions API for new projects and presents `generateContent` as legacy but still fully
supported. The documented image flow posts typed `input` and optional `response_format` to
`POST /v1beta/interactions`; a subsequent edit supplies `previous_interaction_id`. Raw REST output
is typed content inside `interaction.steps`, while `interaction.output_image` is an SDK-added
convenience for the last image, not a REST wire field.

The repository does not implement Interactions. Its native Gemini plane implements only model
listing/get, `generateContent`, `streamGenerateContent`, and `countTokens`. The router's
`/v1beta/{*rest}` wildcard merely dispatches requests to that plane; the plane's closed parser is
the actual support boundary (`crates/router/src/main.rs:79-99`,
`crates/forward/src/gemini/api.rs:452-493`). Consequently, current
`client.interactions.create(...)` examples fail rather than receive an Interaction resource.

Image generation and single-request reference-image transformation are available only through the
repository's constrained native `generateContent` subset for `gemini-3.1-flash-image`. Generated
media survives there as nested `candidates[].content.parts[].inlineData`. The OpenAI Chat and
Responses adapters accept bounded inline image **input**, but neither has a generated-image output
mapping: they discard Gemini `inlineData` and real `thoughtSignature` parts after native settlement.
This creates a material failure mode in which upstream image use can be settled while no media is
delivered to the OpenAI-compatible caller.

## Capability matrix

| Capability | Google documentation reviewed 2026-08-05 | Repository at recorded SHA | Verdict |
|---|---|---|---|
| Interactions create | `POST /v1beta/interactions`; typed `input`; optional `response_format` | No route or handler; native parser has a closed model-method set | Unsupported |
| Stateful multi-turn image edit | Continue a completed turn with `previous_interaction_id` | No Interaction resource or server-side conversation store | Unsupported |
| Native image generation/editing | Interactions preferred; `generateContent` remains supported | Constrained `generateContent`/stream subset for `gemini-3.1-flash-image` | Supported subset |
| Native generated-image output | Typed `model_output` content in `interaction.steps`; SDK convenience `interaction.output_image` | Candidate part `inlineData` | Supported only on native Gemini response shape |
| Native streaming | Interactions typed execution stream; legacy native stream remains documented | Private SSE translated to downstream SSE for `alt=sse`, otherwise JSON array | Supported legacy subset |
| Official SDK | Current SDKs expose `client.interactions.create`; core model methods remain available | Repository examples and routes support only relevant `client.models.generate_content` family methods | Partial method compatibility |
| OpenAI-compatible image input | Not the native Google contract | Chat `image_url` and Responses `input_image` accept inline base64 data URLs | Supported input subset |
| OpenAI-compatible generated image | N/A | Generated `inlineData` has no Chat/Responses output mapping | Unsupported; media discarded |
| Affinity | Interactions can store conversation state | Keyed profile/cache affinity only; no image provider session or content store | Not conversation state |

## 1. Current official Interactions contract

The Interactions overview says the API became generally available in June 2026, recommends it for
new projects, and says `generateContent` remains fully supported. The image guide's current REST
shape is:

- `POST /v1beta/interactions`;
- top-level `model` and required typed `input` (for example `{"type":"text","text":"..."}` or
  `{"type":"image","mime_type":"image/png","data":"<base64>"}`);
- optional top-level `response_format`, including image format, aspect ratio, and image size;
- `previous_interaction_id` to continue a completed prior interaction without replaying all history.

An Interaction is a resource with status and an execution timeline. Raw generated image bytes are
found by iterating `interaction.steps[]`, selecting `type: "model_output"`, then typed image content
and its `data`/`mime_type`. The SDK's `interaction.output_image` returns the last generated image as
a convenience; Google explicitly marks it as SDK-added, so a proxy must not invent it as a REST
response field. Stateful continuation preserves prior input/output history, but interaction-scoped
settings still need to be repeated. Google also documents stateless operation by sending complete
history with storage disabled.

This shape is not an alias for `generateContent`. The latter posts
`contents[].parts[]` to `/v1beta/models/{model}:generateContent` and returns
`candidates[].content.parts[]`; Interactions uses typed inputs, `response_format`, Interaction IDs,
and typed execution steps.

## 2. Implemented native Gemini subset

The model resource itself advertises only `generateContent`, `streamGenerateContent`, and
`countTokens` (`crates/forward/src/gemini/api.rs:514-545`). The exact native surface documented in
the repository is list/get models plus those three model methods
(`docs/engine/GEMINI_PROVIDER.md:416-427`). Unsupported methods and resources, including Files, are
rejected by the closed parser and tests (`crates/forward/src/gemini/api.rs:452-493`,
`crates/forward/src/gemini/api.rs:6729-6748`). The broad router wildcard only chooses the Gemini
plane (`crates/router/src/main.rs:96-99`, `crates/router/src/main.rs:158-166`); it does not make every
matching Google path an implemented API.

For `gemini-3.1-flash-image`, the core native route has these exact local controls and limits:

- it forces one candidate and exact text-plus-image output; omitted image settings default to `1:1`
  and `1K` (`crates/forward/src/gemini/api.rs:814-850`);
- accepted ratios are `1:1`, `1:4`, `1:8`, `2:3`, `3:2`, `3:4`, `4:1`, `4:3`, `4:5`, `5:4`,
  `8:1`, `9:16`, `16:9`, and `21:9` (`crates/forward/src/gemini/api.rs:852-855`);
- accepted sizes are `1K`, `2K`, and `4K`; Google's documented `0.5K` is deliberately rejected
  because the private subscription route did not accept it (`crates/forward/src/gemini/api.rs:1174-1186`);
- `maxOutputTokens` must be an integer from 1 through the model's 32,768-token output limit, and
  `candidateCount`, if present, must equal one (`crates/forward/src/gemini/api.rs:1101-1119`);
- there must be nonempty `contents` and at least one nonblank text prompt
  (`crates/forward/src/gemini/api.rs:1072-1093`);
- at most 14 inline references are accepted; MIME is limited to PNG, JPEG, WEBP, HEIC, or HEIF;
  data must be nonempty valid base64; decoded inline images have a 20 MiB aggregate cap
  (`crates/forward/src/gemini/api.rs:1191-1226`);
- the raw image request body is independently capped at 20 MiB and response/pending stream data at
  64 MiB (`crates/forward/src/gemini/api.rs:27-32`);
- `fileData` is rejected because credential rotation cannot preserve project-scoped files
  (`crates/forward/src/gemini/api.rs:1228-1236`), and the repository exposes neither a Files API nor
  an image-route video/operation lifecycle;
- system instructions and all tools, including Google Search, fail closed on this image route
  (`crates/forward/src/gemini/api.rs:1058-1070`); generic tool support elsewhere is not image-model
  support;
- thinking, structured-output and response-MIME/schema/logprob controls are rejected
  (`crates/forward/src/gemini/api.rs:1138-1151`), as are image-config fields beyond ratio and size;
- native output retains the complete candidate objects, so generated `inlineData`, safety/finish
  metadata, and genuine part-level `thoughtSignature` survive; only private/unknown top-level wrapper
  fields are removed before a new public `responseId` is synthesized
  (`crates/forward/src/gemini/api.rs:1515-1576`).

`streamGenerateContent` requests are sent upstream as SSE. The gateway incrementally emits Gemini
SSE when `alt=sse`, or the native JSON-array form by default/with `alt=json`; generated images remain
base64 candidate parts rather than a separate file (`docs/engine/GEMINI_PROVIDER.md:421-425`,
`crates/forward/src/gemini/api.rs:1867-1988`). The image response limit is therefore material for
both non-stream bodies and pending stream frames.

The gateway admits caller `safetySettings` and preserves provider-returned safety metadata, but it
has no independent image safety classifier. Google's image guide says all generated images carry
SynthID; no local SynthID application or verification exists. Safety decisions and SynthID are
therefore upstream-dependent claims, not gateway-enforced guarantees. This audit did not make a
live call to re-prove either behavior.

## 3. Multi-turn and affinity boundary

Google's documented multi-turn editing depends on an Interaction resource and
`previous_interaction_id`. Neither exists here. Replaying the complete native `contents` history,
including prior model image parts, is a possible **client-carried adaptation** to stateless
`generateContent`; it is not equivalent to Interactions and has not been live-proven in this audit
for image fidelity or genuine thought-signature replay.

Affinity must not be described as conversation storage. The gateway hashes ordered native contents
and selected controls into a tenant-scoped profile/cache lineage; only keyed digests are persisted
(`crates/forward/src/affinity.rs:434-460`). Image requests can use that result to prefer a warm
subscription, but their private Antigravity identity explicitly carries no image `sessionId`
(`crates/forward/src/gemini/api.rs:1431-1453`). Affinity neither stores the image conversation nor
allows the server to reconstruct omitted turns. A caller must resend every prompt/reference/history
needed by the native stateless request.

## 4. OpenAI compatibility: input is not output

### Accepted input

Chat Completions maps a user `image_url` part only when its URL is an inline
`data:image/...;base64,...` value. Remote HTTP(S), `file:` URLs, raw unwrapped base64, empty/non-image
data URLs, and non-`auto` detail values are rejected; no Files API ID is recognized
(`crates/forward/src/gemini/chat.rs:585-628`). Responses maps `input_image.image_url` through the
same helper and rejects unknown parts, including `input_file`/`file_id`
(`crates/forward/src/gemini/responses.rs:680-731`). These are image **input** adapters, not generated
media support.

### Lost output and signatures

The Chat output converter emits only thought text, normal text, and function calls. Candidate
`inlineData` and signature-only parts have no representation and are skipped in both non-stream and
stream conversion (`crates/forward/src/gemini/chat.rs:1309-1357`,
`crates/forward/src/gemini/chat.rs:1547-1598`). Responses likewise emits reasoning-summary text,
message text, and function-call items, explicitly skipping unknown parts
(`crates/forward/src/gemini/responses.rs:1025-1083`,
`crates/forward/src/gemini/responses.rs:1613-1693`). Both skins discard real
`thoughtSignature`; tool-history reconstruction uses a synthetic stateless marker instead
(`crates/forward/CLAUDE.md:407-419`, `crates/forward/CLAUDE.md:427-449`).

Settlement examines the native response before the outer conversion. It detects delivered images
from candidate `inlineData`, prefers provider modality usage, and otherwise applies the
size-dependent image split only when media was actually delivered
(`crates/forward/src/gemini/api.rs:1634-1672`,
`crates/forward/src/gemini/api.rs:1816-1827`). Thus a Chat or Responses call can settle real upstream
image usage while the adapter returns no image. This is a static composed-code finding; the recorded
SHA has no end-to-end regression test proving that exact charged-but-dropped composition.

## 5. SDK compatibility boundary

The official Google GenAI SDK is compatible here only for the implemented core model-method subset.
Repository examples using `client.models.generate_content(...)`/`models.generateContent(...)` map to
the native `generateContent` route. Model discovery, supported streaming, and token counting map to
the other implemented methods. Current official image examples using
`client.interactions.create(...)` do not: `/v1beta/interactions` reaches the Gemini plane but fails
the closed native parser. Files, uploads, batches, explicit cache resources, and other SDK services
are likewise not implied by the existence of a configurable base URL.

## 6. Documentation discrepancy register

This audit records discrepancies but intentionally does **not** edit runtime or product copy.

| ID | Severity | Current wording/surface | Discrepancy |
|---|---|---|---|
| GI-01 | High | Native lanes are described as "byte-faithful", "unchanged", or "as-is" (`apps/web/src/lib/md-pages.ts:404`, `apps/web/src/app/docs/api-reference-data.ts:255-258`, `apps/web/src/lib/llms.ts:146`) | The gateway implements a reconstructed, validated subset, synthesizes IDs, filters fields, and has model-specific rejections. |
| GI-02 | High | "Everything the provider APIs support passes through unchanged" and identical tools/multi-turn/system controls (`apps/web/src/lib/md-pages.ts:524-532`) | Interactions, Files, image tools/Search, image system instructions, structured output, thinking controls, and multiple candidates are not supported. |
| GI-03 | High | "Reuse the official SDKs unchanged" / "any Gemini-compatible tool" (`apps/web/src/lib/md-pages.ts:488-522`, `apps/web/src/lib/models.ts:778-781`) | Compatibility is method-specific: core `models.generate_content` works; current `interactions.create` and other absent services fail. |
| GI-04 | High | Image generation/editing advertised without its transport qualifier (`apps/web/src/lib/models.ts:762-781`) | Editing is single-request inline-reference transformation on the native core route; documented stateful Interactions editing is absent. |
| GI-05 | High | Catalog-level modality wording says Nano Banana outputs images while one client protocol reaches the catalog (`apps/web/src/lib/md-pages.ts:419-421`) | Generated media reaches native Gemini callers only; OpenAI Chat/Responses drop it after settlement. |
| GI-06 | Medium | General affinity and multi-turn language can imply stored image continuity (`docs/engine/GEMINI_PROVIDER.md:464-476`) | Image affinity chooses a warm profile but sends no provider image session and stores no conversation. |
| GI-07 | Medium | Existing calibration plan has paid 1K/2K/4K image legs | Its builder uses `responseModalities:["IMAGE"]`, while runtime admission requires exact TEXT+IMAGE, so those legs cannot currently prove live image generation (`tools/gemini_calibration/run_live.py:830-837`, `crates/forward/src/gemini/api.rs:1120-1135`). |

The engine provider document is materially more accurate than the public copy: it lists the closed
route set, native `inlineData` output, OpenCode media loss, image defaults/limits, and stateless image
identity (`docs/engine/GEMINI_PROVIDER.md:416-427`, `docs/engine/GEMINI_PROVIDER.md:578-633`). Its
phrase "indistinguishable" at `docs/engine/GEMINI_PROVIDER.md:455-462` is nevertheless broader than
the transformations documented immediately beneath it.

## 7. Implications and remediation boundary

The safest current product contract is:

1. advertise **native Gemini core image generation and single-request inline-reference editing**, not
   Interactions or stateful multi-turn image editing;
2. qualify official SDK support by method: `client.models.generate_content` and the other listed core
   methods only;
3. state that generated images are delivered only by the native Gemini route and that Chat/Responses
   image acceptance describes input, not output;
4. never present affinity as conversation storage or a substitute for `previous_interaction_id`;
5. fix the calibration request shape before treating its paid image legs as evidence;
6. if Interactions is implemented later, treat it as a new API contract with typed input/output,
   state retention/deletion semantics, streaming steps, usage settlement, media limits, and SDK/REST
   parity tests—not as a wildcard-router change;
7. before exposing generated images through OpenAI skins, add explicit output mappings and an
   end-to-end test that binds delivered media to authoritative settlement and preserves or rejects
   opaque replay artifacts rather than silently deleting them.

No runtime, product documentation, public copy, pricing, model catalog, provider contract, or
`docs/DEPENDENCIES.md` change is made by this audit-only commit. Remediation should be separately
scoped and live-verified where it claims provider behavior. This report remains a dated append-only
snapshot even after those changes land.
