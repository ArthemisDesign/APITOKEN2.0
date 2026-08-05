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
  `012fccc471142fc51a46563da3a87564d674b39f` is watchdog-GREEN. Its distinct bounded attempt in
  delivery `d7b394fc5e6b9b603e1e0ab3982038f5479ba2e8` reached a parsed image but returned control
  metadata that did not exactly match `opaque/low/1024x1024`. It recorded
  `evidence_controls_mismatch`, published no PNG/checkpoint, and is permanently fenced without replay.
  Because the terminal journal retained only the mismatch class—not returned controls, image, or usage—
  the attempt is withdrawn rather than partial evidence. No owned live edit was performed.

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
| [GPT Image 2 token calculator](https://developers.openai.com/_astro/GptImage2TokenCalculator.react.Bs2iBXlE.js) | Exact output-token formula and request-valid dimension constraints used for the exhaustive low-quality ceiling | Published site implementation; replacement-price budgeting only |
| [Codex issue #28723](https://github.com/openai/codex/issues/28723) | OAuth/Codex image generation silently normalizes explicit size and quality to auto; observed outputs include `1254x1254` | Upstream field report, corroborated by our owned production result; not an API guarantee |
| [Codex issue #19175](https://github.com/openai/codex/issues/19175) | Built-in image generation lacks deterministic dimensions | Upstream limitation report; supports fail-closed omission of exact-size claims |

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

The private canary narrows those controls to `opaque`, `low`, `auto`, one PNG output. Production
returned `1254x1254` after an explicit `1024x1024` request, matching the upstream reports that native
Codex OAuth converts image dimensions to automatic selection. The canary therefore validates only a
bounded auto-size envelope and accepts returned `size` metadata of `auto` or the decoded PNG's exact
`WIDTHxHEIGHT`; it does not claim deterministic dimensions. The reusable transport remains private.
No mask, partial-image streaming, JPEG/WebP, compression, output count, moderation level, Responses
image tool, file-id input, or multi-turn image state is exposed. `input_fidelity` is also absent:
official GPT Image 2 always uses high fidelity and does not allow the caller to change it.

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
usage as opaque provider evidence. The canary is stricter and accepts evidence only when usage is
present, metadata is `opaque/low`, and the native auto output stays within maximum edge 3840,
655,360–8,294,400 pixels and aspect ratio 3:1.

The upstream fixture currently shows medium `1024x1536` usage with 1,474 input tokens (1,457 image,
17 text) and 1,372 output tokens. This is useful schema evidence, not a live cost or quota claim.
Nothing here converts API replacement nanoUSD into ChatGPT credits or customer settlement.

## Generation budget boundary

For low quality, the official GPT Image 2 calculator uses a 16-cell long-side grid, rounds the
short-side grid by aspect ratio, and applies the pixel-dependent multiplier. Exhaustively evaluating
all request-valid resolutions gives a maximum of 659 output tokens at `2880x2880`; reference vectors
are `1024x1024 → 196` and `3840x2160 → 371`. At the official `$30/M` image-output rate this is
`$0.01977`. Treating all 512 allowed prompt bytes as fresh text tokens at `$5/M` adds `$0.00256`, so
the enforced generation authorization ceiling is `$0.02233` (`22_330_000` nanoUSD).

This is an auto-size replacement-price ceiling, not a ChatGPT native-credit reserve. Paid generation
requires that concrete integer budget and an exact compile-time implementation SHA. A free
`/wham/usage` preflight precedes dispatch but is not image tokenization or generation evidence.

Edit remains blocked. GPT Image 2 always processes references at high fidelity, and neither PNG bytes
nor the generation fixture provides a normative maximum input-token formula. A larger arbitrary
budget cannot unlock edit. First obtain real terminal auto-size generation evidence and a reviewed edit
ceiling; then authorize one owned-reference edit separately.

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

1. The RED production baseline is closed by a non-network verifier for the terminal
   `evidence_controls_mismatch` attempt; never replay that image turn or treat it as successful evidence.
2. The private recovery change is deployed under watchdog-GREEN SHA
   `8fcd7c3c6f5dc968bedb7260433f2eaff23f8931`: a rejected parsed result records sanitized returned
   controls, dimensions, identity flags, usage, optional request id, and digest—but not image bytes—in
   the mode-0600 journal. This diagnostic evidence is not publication evidence. Active watchdog-GREEN
   descendant `3c17b31b6dfdcb8867d8def57e7aedc4ebc87644` has an empty diff from that SHA across
   `openai_image_canary.rs` and the three Codex image transport/config files.
3. Delivery `237a926b054a5fdd6833fca6668040ab6e0d55a7` ran the separately authorized one-shot against
   exact active SHA `3c17b31b6dfdcb8867d8def57e7aedc4ebc87644`. The sealed native endpoint returned exact
   home and turn, opaque/low PNG metadata, terminal usage (`35` input, `229` image output, `264` total),
   and `1254x1254` instead of requested `1024x1024`. No image or checkpoint was persisted or published;
   the mode-0600 journal records `evidence_controls_mismatch` and the output digest. The attempt is
   terminal and must not be replayed. A jq syntax error in the optional-usage verifier caused the delivery
   itself to report RED; its correction may only consume this existing journal before credential loading
   and network dispatch. This proves native generation reachability and authoritative usage, but not the
   deterministic size control required for publication.
4. Auto-size implementation SHA `df58715abb4f1ac52b6c46b1ea6f830c6e11178f` is independently
   watchdog-GREEN. Its separate one-shot controller is pinned to that exact active binary, a fresh
   SHA-keyed root and `22_330_000` nanoUSD. It can make one `opaque/low/auto` exact-home generation
   through the sealed pool after the free `/wham/usage` preflight; both external fallbacks remain forced
   off. Only a bounded PNG with terminal usage and exact home/turn/SHA evidence is GREEN.
5. Derive and review a normative edit ceiling, authorize it separately, and run an exact-home edit with
   an owned generated PNG. Verify every claimed reference/edit behavior.
6. Resolve partial-image streaming for the actual native subscription wire before claiming it. Public
   API documentation alone is insufficient.
7. Only after GREEN generation and edit implement producer-first image billing/customer routes, then a
   separate authenticated public production smoke.
8. Only in a later publication commit update contracts, model catalogs, router presets, pricing
   releases, OpenKeys, website, public docs, admin and production defaults. Unsupported mask, streaming,
   format, and multi-turn controls remain absent and explicitly rejected.

Until every applicable gate closes, GPT Image 2 remains private and unpublished.
