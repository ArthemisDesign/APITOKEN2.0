# Claude Haiku 4.5 — live access blockers (investigation, 2026-08-06)

## Question

The site (`apps/web`) advertises seven Claude models on one key and balance. Which of them do we
promise to clients but the engine does not actually serve, and why can clients not use
`claude-haiku-4-5` (reproduced from OpenCode)?

## Method

- Static analysis of the router (`crates/router`), engine admission (`crates/forward`,
  `crates/metering`) and the site catalog (`apps/web/src/lib/models.ts`).
- Live production state on the commercial host: engine PostgreSQL (`claude_engine`, catalog heads,
  pricing entries, per-account policy versions, request snapshots, shadow evaluations) and the
  Control API (`GET /admin/pricing/catalog/main/active`).
- Live API probes: the router public catalog (`router.apitoken.sale/v1/models`) and direct upstream
  `POST /v1/messages/count_tokens` against `api.anthropic.com` with a real pool subscription token
  and full Claude Code persona headers. No money was spent; only free/read-only endpoints were hit.

## What the engine has (all green)

- Active head catalog is **generation 5** (products `main` and `openkeys`, activated 2026-08-04);
  all **seven** advertised Claude models are present and `enabled=true` — including
  `claude-haiku-4-5`. Live Control API response matches the compile-time capability manifest
  digest in `crates/forward/src/pricing.rs`.
- Metering tariffs exist for all seven models and match the site price list
  (`crates/metering/src/lib.rs`); Sonnet 5 is billed at the intro $2/$10 until 2026-08-31.

## Production traffic — the mismatch

All-time and recent admitted requests by model (`pricing_request_snapshots_v2`, plus legacy
`pricing_admission_snapshots` and `pricing_shadow_admission_evaluations`):

| canonical_model_id | requests | last seen |
|---|---|---|
| claude-opus-5    | 944  | 2026-08-05 |
| claude-sonnet-5  | 486  | 2026-08-05 |
| claude-opus-4-8  | 167  | 2026-08-05 |
| claude-fable-5   | 115  | 2026-08-05 |
| claude-sonnet-4-6|  40  | 2026-08-05 |
| claude-opus-4-7  |  31  | 2026-08-05 |
| **claude-haiku-4-5** | **0** | — |

`claude-haiku-4-5` has zero admitted requests **ever** (bare and dated `claude-haiku-4-5-20251001`
alike): no v2 snapshots, no legacy snapshots, no shadow evaluations, no engine-log mentions. Every
other advertised Claude model demonstrably runs. The engine catalog is not the gate — requests die
before admission.

## Root cause — two independent blockers

### Blocker A: the advertised bare ID is absent from the catalog

- The site and learn articles tell users to send `claude-haiku-4-5`. The live unified catalog
  (`router.apitoken.sale/v1/models`) publishes only `anthropic/claude-haiku-4-5-20251001`; the
  entry's `aliases` equals its own id (upstream returns no bare alias, unlike `claude-opus-5`).
- `GET /v1/models/claude-haiku-4-5` → 404; a request carrying the bare id is refused by the router
  as an unknown model (`crates/router/src/routing.rs` catalog lookup), so the documented ID can
  never work through the router.
- Claude Code and the OpenCode plugin discover the dated id from `/v1/models` and send that one —
  straight into Blocker B.

### Blocker B: engine admission rejects the dated ID (the one clients actually send)

- The release-v2 pricing bridge canonicalizes Anthropic models via
  `metering::anthropic_tariff_capability_at` (`crates/metering/src/lib.rs`), which recognizes
  **only the seven bare ids** — `claude-haiku-4-5-20251001` is not among them.
- For the dated id the bridge returns `UnsupportedModelIdentity` → `PricingBridgePrepare::Fallback`
  → `reserve_anthropic_release_v2` bails (`crates/forward/src/proxy.rs`) → the request fails with
  **503 `pricing_release_admission_unavailable`** before reserve, before any snapshot, before any
  upstream call. This is exactly what OpenCode (and Claude Code) hit with the dated id.
- The bare id would pass admission, but clients cannot send it (Blocker A). Net result: the model
  is 100% unusable through every client path despite being advertised, catalogued and tariffed.

## Bonus finding: `count_tokens` is broken for ALL models

`POST /v1/messages/count_tokens` returns `"The request could not be processed by the selected
model."` (HTTP 200 with error body) for every model — verified through the router, through the
engine, and directly against `api.anthropic.com` with a live subscription token and the full
Claude Code persona header set (opus-5, sonnet-5, opus-4-8 and haiku fail identically). The pool's
own probe (`crates/forward/src/upstream.rs` `poll_sub`) only reads rate-limit headers off the
response, so the pool stays healthy and the breakage is invisible. Any SDK / Claude Code prompt
size estimation is silently failing product-wide. Separate ticket.

## Fix options (not applied)

1. Engine: canonicalize `claude-haiku-4-5-20251001` (and dated aliases in general) to
   `claude-haiku-4-5` in `anthropic_tariff_capability_at`, or rewrite dated → bare before
   admission. This alone unblocks the dated id that clients actually send.
2. Catalog: publish the bare `claude-haiku-4-5` alias (upstream alias passthrough or router-side
   alias) so the site-documented ID works and `GET /v1/models/claude-haiku-4-5` stops 404ing.
3. Investigate upstream `count_tokens` support on the subscription identity; if it stays broken,
   add a local tokenizer fallback instead of forwarding.

## Facts vs assumptions

Verified live: catalog generation, per-model admission counts (zero for haiku), router catalog
content, the 503 code path in the release-v2 reserve, and the universal count_tokens upstream
rejection. Not verified (would spend money): a real generation attempt with the dated haiku id —
per the code path it cannot pass admission anyway.
