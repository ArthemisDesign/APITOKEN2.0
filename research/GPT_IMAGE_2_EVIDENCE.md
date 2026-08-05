# GPT Image 2 on the current OpenAI Codex OAuth wire — private evidence

## Verdict

- Model: `gpt-image-2`; official immutable snapshot: `gpt-image-2-2026-04-21`.
- Native subscription endpoints: `POST {CodexConfig.base_url}/images/generations` and
  `POST {CodexConfig.base_url}/images/edits`.
- Authentication remains the existing sealed ChatGPT Codex OAuth profile, account id, proxy, refresh
  family, official-client identity, and pool. There is no relay, image API key, image origin, or
  image-specific environment variable.
- The private transport and canary exist, but there is no customer HTTP route, catalog, router preset,
  billing/settlement, production default, storefront, or publication claim.
- Initial implementation SHA `3f67d43c0ae541979fee66823d251e2e3eea33e0` is deployed and
  watchdog-GREEN. Controller delivery `7a334604f41c367e898073567a7aa0d481614839` stopped before network
  access because systemd `EnvironmentFile` syntax was sourced as Bash; no paid dispatch occurred.
  Delivery `2c7dabcce85be9a691597d6b5ab765fe4868a3b6` then reached a parsed image result but withheld it
  because the provider supplied no optional request-id header; that exact attempt is permanently
  fenced and never replayed. Corrected implementation SHA
  `012fccc471142fc51a46563da3a87564d674b39f` is watchdog-GREEN, and its distinct bounded
  production generation gate is prepared but not yet live-proven; no owned live edit was performed.
  Mock tests and source review are not live proof.

## Official and upstream sources

The current upstream revision reviewed here is OpenAI Codex
[`9d00bb01c0a712fb7c2f5b002bdf33bcc0fc352c`](https://github.com/openai/codex/tree/9d00bb01c0a712fb7c2f5b002bdf33bcc0fc352c).

| Source | Evidence | Boundary |
|---|---|---|
| [GPT Image 2 model reference](https://developers.openai.com/api/docs/models/gpt-image-2) | Alias/snapshot, text+image input, image output, generation/edit endpoints | Public OpenAI API contract; not ChatGPT subscription acceptance or native-credit authority |
| [OpenAI image generation guide](https://developers.openai.com/api/docs/guides/image-generation) | Generation, edit, references, masks, high-fidelity input, sizes, qualities, formats, partial-image streaming, and pricing | Broader public API/Responses surface; unsupported native Codex fields cannot be inferred from it |
| [Codex typed image requests](https://github.com/openai/codex/blob/9d00bb01c0a712fb7c2f5b002bdf33bcc0fc352c/codex-rs/codex-api/src/images.rs) | Native generation/edit JSON fields are prompt/images, background, model, n, quality, size | No mask, output format/compression, partial images, or input-fidelity field |
| [Codex Images endpoint client](https://github.com/openai/codex/blob/9d00bb01c0a712fb7c2f5b002bdf33bcc0fc352c/codex-rs/codex-api/src/endpoint/images.rs) | Ordinary JSON POST to provider-relative `images/generations|edits`; fixture includes base64 output and terminal usage and succeeds with an empty response-header map | Fixture is not an owned production result; client uses `execute`, not streaming, and provider request-id is not response authority |
| [OpenAI API pricing](https://developers.openai.com/api/docs/pricing/) | Official text/image input and image output replacement rates | Not ChatGPT subscription credits, quota, or customer billing authority |

The official guide currently says Image API and Responses API can stream 0–3 partial images. That does
not contradict the local fail-closed decision: the native Codex client reviewed above has no
`partial_images` request field and performs a non-streaming `execute`. Public API capability is not
proof of subscription-wire capability.

## Native wire implemented

The local transport follows the current native shape:

- generation JSON: prompt, `model: "gpt-image-2"`, typed `background`, `quality`, and `size`;
- edit JSON: the same plus one to five strict PNG data URLs in `images[].image_url`;
- shared headers: existing OAuth bearer, `ChatGPT-Account-ID`, `originator`, pinned Codex
  `User-Agent`, and pinned `version`;
- image correlation: one `x-codex-image-turn-id` retained across the single same-home forced-refresh
  replay.

Typed local controls admit official GPT Image 2 values:

- background: `auto|opaque`; transparency is intentionally absent because GPT Image 2 rejects it;
- quality: `low|medium|high|auto`;
- size: `auto` or exact dimensions with max edge 3840, edges divisible by 16, aspect at most 3:1, and
  655,360–8,294,400 pixels.

The private canary narrows those controls to `opaque`, `low`, `1024x1024`, one PNG output. The reusable
transport remains private. No mask, partial-image streaming, JPEG/WebP, compression, output count,
moderation level, Responses image tool, file-id input, or multi-turn image state is exposed.
`input_fidelity` is also absent: official GPT Image 2 always uses high fidelity and does not allow the
caller to change it.

## Pool, refresh and replay

Image operations reuse the Codex pool instead of creating another provider. Automatic library calls
may rotate only after a final `401/403` or `429` proves an account/quota rejection. The canary is
stricter: it freezes either a supplied opaque id or the first currently admitted id, runs the free
`/wham/usage` OAuth/quota probe, and uses exact-home methods.

A first received `401` allows one forced refresh and one same-home replay with the same turn id. Client
rejection, unexpected status, invalid success, timeout, connection/body failure, and any ambiguous
outcome are terminal. They are never replayed because an image may already have been accepted even when
the response was not observed.

## Response and usage authority

The strict parser requires exactly one bounded base64 PNG, plausible `created`, and bounded
`background`, `quality`, and `size`; optional `output_format` must be `png`. Transport retains optional
usage as opaque provider evidence. The canary is stricter and accepts publication evidence only when
usage is present and the returned controls exactly match `opaque/low/1024x1024`.

The upstream fixture currently shows medium `1024x1536` usage with 1,474 input tokens (1,457 image,
17 text) and 1,372 output tokens. This is useful schema evidence, not a live cost or quota claim.
Nothing here converts API replacement nanoUSD into ChatGPT credits or customer settlement.

## Generation budget boundary

The controlled generation fixes low 1024×1024. The official guide prices its output at `$0.006`.
Treating all 512 allowed prompt bytes as fresh text tokens at `$5/M` adds a conservative `$0.00256`.
The enforced generation authorization ceiling is therefore `$0.00856` (`8_560_000` nanoUSD).

This is a fixed-request replacement-price ceiling, not a ChatGPT native-credit reserve. Paid generation
requires that concrete integer budget and an exact compile-time implementation SHA. A free
`/wham/usage` preflight precedes dispatch but is not image tokenization or generation evidence.

Edit remains blocked. GPT Image 2 always processes references at high fidelity, and neither PNG bytes
nor the generation fixture provides a normative maximum input-token formula. A larger arbitrary
budget cannot unlock edit. First obtain real terminal generation usage and a reviewed edit ceiling;
then authorize one owned-reference edit separately.

## Chinese reseller audit

The reseller survey is market reconnaissance only. None of these sources is authoritative for the
OpenAI or ChatGPT subscription wire, none was given credentials, and none is integrated:

- [Hvoy AI gateway profiles](https://www.hvoy.ai/en/sites/apiwhataicc/) aggregate self-descriptions,
  public records and user votes for Chinese relays that claim GPT Image 2. This can reveal market
  availability but does not establish upstream provenance, controls, usage, privacy, or billing.
- [神马中转 / whataicc](https://github.com/whataicc/gpt-image-2) advertises domestic OpenAI-compatible
  GPT Image 2 relay access. The repository is marketing/integration copy, not independently verifiable
  transport or usage evidence.
- [AIHubProxy](https://www.aihubproxy.com/2026zuixingpt-image-2) advertises domestic direct access and
  broad model coverage. Claims such as discounts and high concurrency are provider marketing.
- [LaoZhang API](https://docs.laozhang.ai/en/api-capabilities/gpt-image-2) publishes a provider-owned
  `$0.03/call` contract. Its own blog distinguishes that from official OpenAI pricing; it cannot define
  our pool's subscription accounting.
- [APIXO](https://apixo.ai/models/gpt-image-2) advertises text-to-image and reference-guided editing,
  with its own asynchronous endpoint, input limits and per-image tiers. Those controls are APIXO's
  product contract, not native Codex proof.

The consistent finding is that resellers introduce another key, origin, custody boundary, pricing
contract and often a different asynchronous or OpenAI-compatible schema. That violates this task's
requirement to generate only through our existing OAuth pool. No third-party image relay is integrated.

## Remaining live and publication gates

1. Land and deploy the corrected engine implementation that treats the provider request-id header as
   optional while retaining mandatory local turn identity, strict PNG, exact controls, and terminal usage.
2. After that exact implementation SHA is watchdog-GREEN, authorize and deliver a new one-shot controller
   with a distinct evidence root and the same conservative `$0.00856` generation ceiling.
3. Require 2xx, one real 1024×1024 PNG, exact returned controls, terminal authoritative usage, local turn
   attribution, private mode-0600 evidence, and no ambiguous replay before overall watchdog GREEN.
4. Derive and review a normative edit ceiling, authorize it separately, and run an exact-home edit with
   the generated owned PNG. Verify every claimed reference/edit behavior.
5. Resolve partial-image streaming for the actual native subscription wire before claiming it. Public
   API documentation alone is insufficient.
6. Only after GREEN generation and edit implement producer-first image billing/customer routes, then a
   separate authenticated public production smoke.
7. Only in a later publication commit update contracts, model catalogs, router presets, pricing
   releases, OpenKeys, website, public docs, admin and production defaults. Unsupported mask, streaming,
   format, and multi-turn controls remain absent and explicitly rejected.

Until every applicable gate closes, GPT Image 2 remains private and unpublished.
