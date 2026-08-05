# GPT Image 2 via APIYI official relay — dormant evidence

## Review scope and verdict

- Upstream model alias: `gpt-image-2`.
- Immutable OpenAI snapshot: `gpt-image-2-2026-04-21`.
- Direct Image API operations in scope: `POST /v1/images/generations` and
  `POST /v1/images/edits`.
- Candidate relay base: `https://api.apiyi.com` (OpenAI-compatible API base:
  `https://api.apiyi.com/v1`).
- Stage status: **Stage 1, default-off/dormant evidence, pure metering, strict private transport,
  and locked dry-run-only planner**. The transport is not connected to runtime/customer routing, and
  the current CLI cannot dispatch it.
- Publication status: **not published**. No production default, route, model catalog, router preset,
  website entry, or customer availability claim is authorized by this document.
- Relay status: APIYI describes this lane as an OpenAI “official relay” built on the official
  channel. This is a vendor claim, **not live verified by this project**. No owned generation,
  edit, streaming, terminal usage, cache split, or billed amount has been observed yet. The private
  planner contract and closed live blockers are in `docs/ops/GPT_IMAGE_2_CANARY.md`.

## Sources reviewed

| Source | Relevant claim | Authority and limitation |
|---|---|---|
| [OpenAI GPT Image 2 model](https://developers.openai.com/api/docs/models/gpt-image-2) | Alias, `gpt-image-2-2026-04-21` snapshot, image generations/edits endpoints; model page marks streaming unsupported | Official model reference; conflicts with the guide's Image API streaming statement and therefore does not prove framing |
| [OpenAI image generation guide](https://developers.openai.com/api/docs/guides/image-generation) | Generations and edits, partial-image streaming, output token calculator, final cost legs | Official guide; does not provide a normative terminal usage JSON schema or reserve ceiling |
| [OpenAI API pricing](https://developers.openai.com/api/docs/pricing/) | GPT Image 2 text/image input, cached input, and image output token rates | Official price authority; rates only, not proof of relay reporting semantics |
| [APIYI GPT Image 2 overview](https://docs.apiyi.com/en/api-capabilities/gpt-image-2/overview) | Claims official relay compatibility, base URL, both endpoints, token usage fields, cached pricing, and default/enterprise groups | Vendor documentation; neither independent nor live evidence, and its operational claims remain unverified |

## Official immutable tariff

OpenAI lists prices per one million tokens. The pure metering schedule identity is
`openai/gpt-image-2/2026-04-21/v1`, with `effective_from = 0`; both the alias and snapshot resolve
to that one immutable schedule.

| Disjoint billing leg | Official rate / 1M tokens | Engine rate (nanoUSD/token) |
|---|---:|---:|
| fresh text input | $5.00 | 5,000 |
| cached text input | $1.25 | 1,250 |
| fresh image input | $8.00 | 8,000 |
| cached image input | $2.00 | 2,000 |
| image output | $30.00 | 30,000 |

If upstream reports authoritative cached subsets, metering retains total text and image input plus
those subsets, validates each cached count is no greater than its corresponding total, and derives
fresh input by subtraction. If an authoritative subset is absent, all corresponding input is charged
fresh; no discount is invented. The five priced legs are disjoint, so cached input is never charged
once at the fresh rate and again at the cached rate. The candidate shape comes from the vendor usage
description and must be replaced or confirmed by sanitized live terminal evidence before runtime
settlement is enabled.

## Contract contradictions and unresolved authority

### Streaming

- The official GPT Image 2 model page says **Streaming: Not supported**.
- The official image generation guide says the Responses API and Image API support streaming image
  generation and documents `partial_images`.
- APIYI describes synchronous Images endpoints and does not establish the exact streamed response
  framing or terminal usage authority for this relay.

Chosen Stage 1 behavior: no streaming capability is claimed or published. A controlled live test
must prove incremental partial-image delivery and a terminal authoritative usage record on the
exact candidate transport before any streaming surface can be enabled.

### Usage shape

- OpenAI pricing and the guide establish billable dimensions, but the reviewed official pages do
  not define a complete Images API `usage` JSON schema or whether every successful response includes
  it.
- APIYI claims both endpoints return `usage`, with total input and separate text/image details, but
  this is vendor documentation rather than owned wire evidence.
- The vendor example says output detail mirrors image output and text output is zero; the model rate
  card has no text-output leg. Any unexpected non-image output dimension must fail closed rather
  than be silently ignored.

Chosen Stage 1 behavior: the dormant transport implements a strict candidate parser for the documented
terminal schema and rejects missing, malformed, contradictory, or unpriced dimensions. `created` is
mandatory and must be a Unix timestamp between 2020-01-01 and 2100-01-01 inclusive. The optional
`background`, `output_format`, `quality`, and `size` members may be absent, but an explicit `null` is
invalid and any present value must exactly match the fixed request. Unknown success-envelope fields are
also rejected. This is tested against mocks only. A future separately reviewed live probe must confirm or
invalidate that schema with sanitized evidence before any runtime settlement is considered; the current
CLI cannot run that probe.

### Cached-input split

- OpenAI publishes distinct cached text and cached image rates.
- The reviewed official guide does not show where cached text/image subsets appear in Image API
  terminal usage.
- APIYI says cached pricing is configured but cache hits can be limited by account fan-out; its
  documented usage field list does not establish an authoritative cached text/image split.

Chosen Stage 1 behavior: totals and cached subsets are explicit typed inputs; `cached > total` is
invalid. Runtime integration remains blocked until live evidence identifies exact fields. If the
provider reports no authoritative cached split, settlement must not invent a discount.

### Reserve ceiling

- Output cost depends on generated image tokens, which vary with requested size, quality, and partial
  images. The official guide provides examples/calculators but no normative maximum output-token
  ceiling for every valid GPT Image 2 request.
- APIYI provides estimates and extrapolations, not an official hard token ceiling. It also documents
  a `1.2x` enterprise group, while this Stage 1 tariff records only the official-list-price/default
  `1.0x` lane.
- Input image tokens depend on edit inputs and provider tokenization; no reviewed source supplies a
  complete request-level maximum suitable for a guaranteed hold.

Chosen Stage 1 behavior: no reserve formula or production admission is added. Enabling transport is
blocked on a conservative, tested request-bound reserve ceiling (including image count, dimensions,
quality, partial-image surcharge, and relay group) that provably bounds terminal settlement.

## Stage gate and blockers

The dormant tariff and dry-run plan are evidence only and grant no product access. The current CLI
cannot advance this gate. A separate reviewed live-capable change requires an owned credential and an
exact clean GREEN implementation SHA, and must then:

1. Confirm APIYI's official-relay claim and exact `https://api.apiyi.com/v1` request path without
   retaining credentials or private payloads.
2. Close the admission blocker before any paid call. The Images surface has no reviewed free
   `countTokens` equivalent, so it requires a separately reviewed image-specific free-preflight
   exception or an actual free preflight; absence of one cannot silently skip this step. Because the
   minimum official-list estimate is about `$0.005885`, obtain explicit approval above the default
   aggregate `$0.0001` cap.
3. Obtain successful generation with real image bytes and terminal authoritative usage; reconcile
   every token leg, the APIYI group multiplier, and the charged relay amount.
4. Obtain a successful edit with an owned synthetic reference image and prove text/image input
   separation plus cached-subset behavior (or prove cache is absent and bill all input fresh).
5. Resolve the streaming contradiction with an incremental partial-image run and terminal usage;
   publication cannot waive the repository's incremental SSE requirement.
6. Define and test a conservative reserve ceiling for all admitted controls before runtime billing.
7. Verify retry/disconnect behavior does not duplicate generation or lose settlement after the
   request becomes billable.
8. Only after the full live gate is green may a separate publication change consider a route,
   catalog, defaults, router presets, public docs, or storefront exposure.

Until all blockers are closed, the model remains default-off, dormant, and unpublished.
