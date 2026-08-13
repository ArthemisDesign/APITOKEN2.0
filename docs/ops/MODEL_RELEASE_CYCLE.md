# Model release cycle — adding a model to the live pricing authority

This is the operator runbook for admitting a new model of an existing provider (the gpt-image-2
path generalized). For a new PROVIDER, start from `docs/engine/PROVIDER_ONBOARDING.md` instead and
expect the pricing-authority extension to be its own multi-commit effort.

Two stages, per the repository's two-stage model rule: the dormant implementation lands first and
proves itself live; publication (storefronts, discovery) is a separate step after a GREEN exact
implementation SHA.

> **Head 55 was the final pricing release; every release/policy route is now removed.** A new
> model gets no capability/catalog generation, release pair, policy document or head CAS. The
> implementation supplies its compiled `crates/metering` tariff and the normal provider route;
> the hot `/admin/pricing/tariffs*` authority can seed or supersede that tariff. Public discovery
> and storefronts remain dormant until exact-SHA live proof passes.
>
> Prices and account discounts are separate live authorities:
>
> - prices: `POST /admin/pricing/tariffs/seed|override`, pinned by reserve and replayed at
>   settlement (`docs/engine/CONTROL_API.md`);
> - discounts: the bounded account default and optional provider rows, delivered by the atomic
>   B2B pricing API and fenced `engine_pricing_jobs` (`docs/commerce/PRICING_MODEL.md`).
>
> The former `/admin/pricing/v2/*`, catalog/switch/policy, Stage 5–9, shadow rollout and
> strict-chain surfaces are absent from the engine, commerce, OpenKeys, client and shared
> contracts. Their database objects are immutable incident evidence, not expand-only callable
> APIs; exact deletion gates are in `docs/ops/PRICING_RETIREMENT.md`.
## Phase A — dormant implementation

1. Metering tariff in `crates/metering` (the provider catalog module, e.g. `openai_image.rs`):
   official pricing link in a comment, exact-rate tests, i128 nanoUSD only. Gate: ALL of
   `cargo test -p metering`.
2. Dormant transport/adapter code in `crates/forward` if the model needs a new wire shape
   (e.g. the Images API). Dormant = no public discovery, router preset, storefront or docs entry.
3. Live proof on the exact implementation SHA: generation 2xx with real output, terminal
   authoritative usage, incremental SSE, and the advertised controls — through the existing
   SHA-pinned live/paid smoke gates (see `docs/ops/GPT_IMAGE_2_CANARY.md` for the pattern). A
   failed generation means withdrawal, not publication "for checking".

## Phase B — pricing the new model (hot, no release advance)

The compiled `crates/metering` tariff from Phase A is the price authority. First read
`GET /admin/pricing/tariffs/compiled`: `seed_safe=true` means the family has one complete compiled
epoch and may be bridged into the hot tariff table from compiled constants — never
operator-typed numbers:

```bash
curl -X POST -H "x-api-key: $CONTROL_KEY" -H 'content-type: application/json' \
  -d '{"created_by":"<actor>","reason":"admit <model>","tariff_family":"<family>"}' \
  http://127.0.0.1:8790/admin/pricing/tariffs/seed
```

The runtime tariff book picks the version up within seconds; reserve pins the selected tariff and
settlement replays it. A later correction is a new version through
`POST /admin/pricing/tariffs/override`. Verify the serving plane prices the model: its
`/internal/router/catalog/pricing` producer must resolve a rate card for it — the router drops
any catalog entry it cannot price.

Never seed a family with `seed_safe=false`. The route returns an atomic 400 before authority access
for both a targeted and an all-family request: one `effective_from=0` row cannot preserve a
multi-epoch schedule. For an effective-dated correction, read the existing head, append the current
payload at the rollout timestamp, then append the next payload at its exact future cutoff. Verify
readback after each POST. If a response is uncertain, GET the rows before retrying because POST
allocates another `head + 1` version. If a current correction is appended after a future row, append
a fresh higher-version future row as well or the current version will win after the cutoff. The
full append-only recovery procedure is in `docs/engine/CONTROL_API.md`.

## Phase C — publication

Only after the live proof and the tariff seed are done:

- `apps/web/src/lib/models.ts` + the docs portal and all storefront copy (grep the old model
  list, do not work from memory);
- OpenKeys: runtime-capable models sell at the account's fixed 1:1 scalar; verify the existing
  issuance contract tests and that no OpenKeys-specific allowlist blocks the route;
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
workaround. Do not recreate the retired orchestrated cycle as a script.
