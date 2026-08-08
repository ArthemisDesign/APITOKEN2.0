# Model release cycle — adding a model to the live pricing authority

This is the operator runbook for admitting a new model of an existing provider (the gpt-image-2
path generalized). For a new PROVIDER, start from `docs/engine/PROVIDER_ONBOARDING.md` instead and
expect the pricing-authority extension to be its own multi-commit effort.

Two stages, per the repository's two-stage model rule: the dormant implementation lands first and
proves itself live; publication (storefronts, discovery) is a separate step after a GREEN exact
implementation SHA.

> **Head 55 is the final pricing release — there is no release advance any more.** A new model
> does NOT get a capability/catalog generation, a release pair, or a head CAS. It is priced by
> the engine's `is_model_unpriced` → exact-legacy-tariff fallthrough (a model with no entry in
> the release-pinned catalog is a product gap, not a fault: the caller falls through to the
> exact legacy tariff — see `crates/registry/src/pricing/release_v2.rs`) plus a hot tariff
> seed/override under `/admin/pricing/tariffs*` ("Hot tariff overrides" in
> `docs/engine/CONTROL_API.md`). The release-advance machinery is deleted, not deprecated: the
> Stage 5 pair-preparation materializer cluster
> (`packages/db/src/pricing-stage5-materializer-v2{,-store,-cli}.ts`), the
> `tools/pricing-next-generation.mjs` constants generator, the release-advance transports in
> `packages/engine-client` (`preparePricingReleaseV2`, `preparePricingReleaseRecoveryLinkV2`,
> `getLatestPricingReleasePolicyV2`, `capturePricingStage8EvidenceV2`,
> `activatePricingReleaseV2`), and the OpenKeys internal pricing-inventory producer
> (`GET /api/internal/pricing/v2/inventory`) are gone. The engine-side producers under
> `/admin/pricing/v2/*` stay as expand-only contract surface with no live commerce consumer;
> the durable release/evidence rows from past cycles stay in the database as immutable
> historical evidence.
>
> **Price and discount changes do NOT use this runbook at all.** Both are hot data:
>
> - Prices: `POST /admin/pricing/tariffs/override` (or the compiled-bridge `.../seed`) on the
>   engine Control API — see "Hot tariff overrides" in `docs/engine/CONTROL_API.md`. The engine's
>   process-wide tariff book picks the new version up within seconds, reserves pin it, and
>   in-flight turns settle at their pinned version.
> - Discounts: the managed pricing-policy endpoints (`/v1/admin/pricing-policies/*`,
>   `/v1/admin/provider-switches` on the commerce admin origin) deliver versioned policy/switch
>   generations through the durable pricing-control jobs; the worker confirms them in seconds.
>
> The multi-hour pricing-release orchestration cycle is **deleted**, not deprecated: the
> orchestrator (`pricing-release-orchestration-v2`), the shadow rollout lane
> (`pricing-shadow-rollout-v2`), the commerce Stage 5/6/8/9 routes (`pricing-stage5-v2`,
> `pricing-stage6-v2`, `pricing-stage8-capture-v2`, `pricing-release-activation-v2`), their
> worker lanes, the admin-panel control room, and the fixed host gates
> (`deploy/pricing-stage56-*`, `deploy/pricing-stage567-*`, `deploy/pricing-stage7-*`) were all
> removed. Reason: once prices moved to hot tariff overrides and discounts to hot policy rules,
> the cycle's only remaining effect was hazardous — its materializer would silently recompile the
> B2C policy at a hardcoded 5000 bps and CAS the engine release head, invalidating the live
> provisioning context. The `packages/db` cycle libraries (orchestration, funding normalization,
> shadow rollout, Stage 8 capture, activation jobs/authority, Stage 8 evidence, catalog-gen2) are
> deleted with it.

## Phase A — dormant implementation

1. Metering tariff in `crates/metering` (the provider catalog module, e.g. `openai_image.rs`):
   official pricing link in a comment, exact-rate tests, i128 nanoUSD only. Gate: ALL of
   `cargo test -p metering`.
2. Dormant transport/adapter code in `crates/forward` if the model needs a new wire shape
   (e.g. the Images API). Dormant = no catalog membership, no product access.
3. Live proof on the exact implementation SHA: generation 2xx with real output, terminal
   authoritative usage, incremental SSE, and the advertised controls — through the existing
   SHA-pinned live/paid smoke gates (see `docs/ops/GPT_IMAGE_2_CANARY.md` for the pattern). A
   failed generation means withdrawal, not publication "for checking".

## Phase B — pricing the new model (hot, no release advance)

The compiled `crates/metering` tariff from Phase A is the price authority. Bridge it into the
hot tariff table from the compiled constants — never operator-typed numbers:

```bash
curl -X POST -H "x-api-key: $CONTROL_KEY" -H 'content-type: application/json' \
  -d '{"created_by":"<actor>","reason":"admit <model>","tariff_family":"<family>"}' \
  http://127.0.0.1:8790/admin/pricing/tariffs/seed
```

The runtime tariff book picks the version up within seconds. Because the release-pinned catalog
has no entry for the new model, reserve/settlement resolve it through the `is_model_unpriced`
fallthrough to exactly this tariff; a later correction is a new version through
`POST /admin/pricing/tariffs/override`. Verify the serving plane prices the model: its
`/internal/router/catalog/pricing` producer must resolve a rate card for it — the router drops
any catalog entry it cannot price.

## Phase C — publication

Only after the live proof and the tariff seed are done:

- `apps/web/src/lib/models.ts` + the docs portal and all storefront copy (grep the old model
  list, do not work from memory);
- OpenKeys: the model sells 1:1 through the OpenKeys catalog generation — check
  `assertOpenKeysCatalog` and its tests;
- `apps/admin` calculator `PRODUCT_CATALOG`;
- `crates/router/routing-presets.json` if the router exposes it;
- **discovery**: the serving plane's `GET /v1/models{,/{id}}` must list the model, and the plane's
  `/internal/router/catalog/pricing` producer must resolve a rate card for it — the router drops
  any catalog entry it cannot price. Both are required: a model that is missing from either is
  invisible to every client that discovers capabilities from the catalog, even while its routes
  accept it. This is not hypothetical — gpt-image-2 shipped that way, and agents reported "no image
  model in this pool" against a working image endpoint. If the model is not routable on the text
  lanes, the lanes must reject it with a `400` naming the right endpoint, never a `404`;
- `docs/commerce/PRICING.md` and the provider doc;
- walk the "New model (in an existing provider)" checklist in `docs/CHANGE_CHECKLISTS.md` in
  full and state the applied items in the commit body.

## What this runbook deliberately does not do

No canary accounts, no traffic stop, no manual per-account migration, no hand-computed digests,
no new capability/catalog/release generation, no edits of existing migrations or fenced operator
state. If a step feels like it needs one of those, the correct move is a producer fix, not a
workaround — and never a manual recompile of a live policy: that hazard is exactly why the
orchestrated cycle was deleted; do not recreate it as a script.
