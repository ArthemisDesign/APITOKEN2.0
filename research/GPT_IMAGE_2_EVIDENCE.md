# GPT Image 2 on the current OpenAI Codex OAuth wire — dormant evidence

## Review scope and verdict

- Model: `gpt-image-2`; official immutable snapshot: `gpt-image-2-2026-04-21`.
- Current native Codex endpoints: `POST {CodexConfig.base_url}/images/generations` and
  `POST {CodexConfig.base_url}/images/edits`.
- Authentication: the existing sealed ChatGPT Codex OAuth profile, account id, proxy, refresh family,
  official-client identity, and pool. There is no image API key, image origin, or image-specific env.
- Stage status: **Stage 1 private/default-off and dormant**. The strict transport and exact-profile CLI
  exist, but there is no `AppState`, HTTP/customer route, catalog, router preset, billing/settlement,
  production default, public documentation, or publication claim.
- Live status: **not performed in this worktree**. Mock tests and source review are implementation
  evidence, not proof that an owned ChatGPT subscription currently completes either operation.

## Official sources reviewed

The current upstream source revision reviewed here is OpenAI Codex
[`c4f42d161ae44a8d696ee9fb595709661979d187`](https://github.com/openai/codex/tree/c4f42d161ae44a8d696ee9fb595709661979d187).

| Source | Current evidence | Limitation |
|---|---|---|
| [OpenAI GPT Image 2 model reference](https://developers.openai.com/api/docs/models/gpt-image-2) | Alias/snapshot, text+image input, image output, generations/edits endpoints, and `Streaming: Not supported` | Public OpenAI API model contract; it does not prove ChatGPT subscription quota, native-credit accounting, or this pool's live acceptance |
| [OpenAI Codex image extension backend](https://github.com/openai/codex/blob/c4f42d161ae44a8d696ee9fb595709661979d187/codex-rs/ext/image-generation/src/backend.rs) | Resolves the active provider/auth, creates `ImagesClient`, and adds `originator` plus `x-codex-image-turn-id` for both generation and edit | Source evidence, not an owned live request |
| [OpenAI Codex Images endpoint client](https://github.com/openai/codex/blob/c4f42d161ae44a8d696ee9fb595709661979d187/codex-rs/codex-api/src/endpoint/images.rs) | JSON `POST` to provider-relative `images/generations` or `images/edits`; tests show edit `images: [{image_url: "data:image/png;base64,..."}]` and a response containing `data[].b64_json` plus token usage | Test fixtures do not prove every production response reports usage or that usage maps to ChatGPT credits |
| [OpenAI image generation guide](https://developers.openai.com/api/docs/guides/image-generation) | Public API generation/edit concepts, controls, response images, and pricing dimensions | Broader than this strict Stage 1 subset; masks, arbitrary controls/formats, and streaming are not admitted here |
| [OpenAI API pricing](https://developers.openai.com/api/docs/pricing/) | Official replacement rates for text/image input, cached subsets, and image output | API replacement tariff only; not ChatGPT native credits, subscription quota, or billing |

## Current native Codex mapping

The official Codex source now has a dedicated Images client instead of requiring a third-party relay or
an API-key Images origin. The local Stage 1 implementation follows that current shape:

- generation: JSON `POST {CodexConfig.base_url}/images/generations`;
- edit: JSON `POST {CodexConfig.base_url}/images/edits`;
- shared headers: existing OAuth bearer, `ChatGPT-Account-ID`, `originator`, pinned Codex
  `User-Agent`, and pinned `version`;
- image-only correlation: one fresh `x-codex-image-turn-id` per logical attempt, retained across the
  single same-home forced-refresh replay;
- generation JSON: `model: "gpt-image-2"`, prompt, `background/quality/size: "auto"`;
- edit JSON: the same fields plus one to five PNG data URLs in
  `images[].image_url`.

The implementation intentionally admits only PNG output and strict PNG edit references. It exposes no
masks, streaming, JPEG/WebP, transparency, arbitrary size/quality/background, input fidelity, multiple
outputs, or other public Image API controls. This is a narrow current-wire probe, not a general Images
API adapter.

## Pool, refresh, and replay semantics

Image operations reuse the existing Codex pool rather than creating another provider:

1. Existing selection chooses a currently admitted home and acquires the normal `TurnSlot`; the
   exact-profile CLI instead names one opaque roster id and refuses to move the paid call elsewhere.
2. The existing access-token path performs single-flight refresh and durable refresh-family rotation.
3. A received `401` permits exactly one forced refresh and one same-home replay with the same image
   turn id; a received `403` is already the final pre-execution auth classification.
4. Only a final `401/403` or `429` is a proved pre-execution account/quota rejection eligible for
   automatic rotation in the reusable automatic API. The exact-profile canary returns it directly.
5. Client rejection, unexpected status, invalid success, timeout, connection/body failure, or other
   outcome ambiguity is terminal. It is never replayed because image generation may already have been
   accepted even when no response was observed.

This policy is stricter than the ordinary text pre-byte retry rule because the JSON Images endpoint has
no incremental client-visible boundary that proves the provider did not start work.

## Response and usage authority

The strict success parser requires one bounded base64 PNG in `data[0]`, a plausible `created` Unix
timestamp, and nonempty bounded `background`, `quality`, and `size`; optional `output_format` must be
`png`. Optional `usage` is retained as opaque provider evidence and the CLI checkpoint keeps only an
allow-listed numeric projection. Missing usage remains missing.

The current OpenAI Codex test fixture includes input/output token details, but source fixtures are not
live authority. Stage 1 therefore does not:

- require usage merely to return a private PNG;
- infer cached text/image subsets;
- apply `metering::openai_image` to a customer balance;
- convert OpenAI API replacement nanoUSD into ChatGPT native credits;
- claim ChatGPT subscription billing or settlement semantics.

A controlled live run must determine whether the native subscription response reports stable terminal
usage and whether `/wham/usage` moves at sufficient resolution to attribute native consumption. Until
then, optional usage is evidence only.

## Dormant official replacement tariff

`crates/metering/src/openai_image.rs` remains pure dormant authority for the official OpenAI API
replacement schedule `openai/gpt-image-2/2026-04-21/v1`:

| Disjoint billing leg | Official rate / 1M tokens | Engine nanoUSD/token |
|---|---:|---:|
| fresh text input | $5.00 | 5,000 |
| cached text input | $1.25 | 1,250 |
| fresh image input | $8.00 | 8,000 |
| cached image input | $2.00 | 2,000 |
| image output | $30.00 | 30,000 |

The alias and immutable snapshot share this schedule. If a future authoritative usage shape contains
cached subsets, each subset must be validated against its total and fresh input derived by subtraction;
otherwise all corresponding input is fresh. These are replacement prices only. They are not a
ChatGPT credit card, native subscription tariff, reserve proof, customer billing authority, or product
publication.

## Free preflight and paid budget boundary

The dormant execution path constructs the configured Codex gateway, runs the existing free profile
auth/quota preflight through `/wham/usage`, and then prepares one exact-profile image call. The preflight
would prove roster OAuth/quota health and normal admission without spending an image turn. It is not
image `countTokens`, does not tokenize references, and does not establish a request reserve.

The CLI validates integer `--budget-nanousd > 100000` (`$0.0001`) and at least its checked estimate:

```text
prompt UTF-8 bytes × fresh text-input rate
+ reference PNG bytes × fresh image-input rate
+ 196 × image-output rate
```

This estimate deliberately overstates image input by treating stored bytes as tokens, but it is still
not a normative maximum and cannot stop an already dispatched request. Because the fixed provider wire
uses `quality:auto,size:auto`, no enforceable worst-case charge bound has been proved. Stage 1 therefore
reports `state: "blocked"`, `executable: false`; `--execute` fails before configuration, `/wham/usage`,
or an image request. The plan records the budget and estimate without claiming a hold or settlement.

## Remaining live and publication blockers

No live proof was produced in this worktree. Before any publication change, all of the following remain:

1. Build an exact clean implementation SHA and obtain GREEN repository/deployment validation for it.
2. Prove an enforceable worst-case bound for the fixed `quality:auto,size:auto` request, land a
   separate reviewed change enabling the Stage 1 execution gate, and obtain explicit authorization for
   a concrete budget above `$0.0001`. Only then run the free `/wham/usage` profile preflight and one
   minimal exact-profile generation, preserving only private `0600` PNG/checkpoint evidence.
3. Prove a real successful generation response, PNG integrity, final metadata, request attribution,
   and whatever terminal usage/native quota movement is actually authoritative. Do not invent missing
   metering or settlement.
4. Separately authorize and run an exact-profile edit with owned synthetic PNG references; verify the
   current JSON data-URL wire up to five inputs and the same no-replay behavior.
5. Establish a tested conservative reserve ceiling before any money admission. The CLI estimate is not
   that ceiling, and `/wham/usage` is not image `countTokens`.
6. Resolve the repository publication requirement for incremental SSE. The official model page says
   streaming is unsupported and this Stage 1 implementation has none; therefore it cannot currently
   pass that generic gate.
7. Only in a later publication commit consider `AppState`/HTTP/customer routes, catalog/pricing
   releases, router presets, defaults/systemd, public docs/storefronts, and real billing/settlement.
   Masks, streaming, JPEG/WebP, and arbitrary controls remain out of scope unless separately proved and
   reviewed.

Until every applicable blocker closes, GPT Image 2 remains private, dormant, and unpublished.
