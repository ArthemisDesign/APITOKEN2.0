# Model release cycle — adding a model to the live pricing authority

This is the operator runbook for admitting a new model of an existing provider (the gpt-image-2
path generalized). For a new PROVIDER, start from `docs/engine/PROVIDER_ONBOARDING.md` instead and
expect the pricing-authority extension to be its own multi-commit effort.

Two phases, per the repository's two-stage model rule: the dormant implementation lands first and
proves itself live; publication (catalogs, release advance, storefronts) is a separate step after
a GREEN exact implementation SHA.

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
> deleted with it; only the Stage 5 pair-preparation materializer cluster
> (`pricing-stage5-materializer-v2{,-store,-cli}.ts`) remains, as the runner for Phase D below.

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

## Phase B — release generation constants (generated, never hand-edited)

All frozen constants for the next capability generation come from the generator:

```bash
node tools/pricing-next-generation.mjs <spec.json>
```

The spec names the current generation and the added entries (provider, canonical model id,
alias, products). The tool prints the exact blocks for every hand-edit site: the
`packages/contracts/src/index.ts` generation constants, both Rust mirrors
(`crates/forward/src/pricing.rs`, `crates/server/src/config.rs`), the Stage 5 materializer
constants (`packages/db/src/pricing-stage5-materializer-v2.ts`), and the engine-client policy
version/OpenKeys rows. Apply the blocks, then prove exactness with the replay test
(`node --test 'tools/**/*.test.mjs'`). A digest typed by hand is a drift incident waiting to
happen; the generator exists because the gpt-image-2 admission hit several.

One commit carries the constants + mirrors + materializer and merges through the standard gate.

## Phase C — catalog/switch delivery (hot, durable)

Once the constants are live and deployed, deliver the new catalog/switch generation to the
engine FIRST through the kept hot delivery routes — the engine rejects policies against an
unknown generation as `missing_dependency` while the heads still point at the previous
generation. Stage the durable delivery jobs through the AdminGuard API (the worker confirms
them in seconds; verify the engine heads moved):

```bash
curl -X POST -H "x-admin-key: $COMMERCIAL_ADMIN_KEY" -H "x-admin-actor: <actor>" \
  -H 'content-type: application/json' \
  -d '{"product_id":"main","generation":<N>,"reason":"..."}' \
  http://127.0.0.1:8791/v1/admin/pricing-catalog-jobs/stage
# repeat for product_id "openkeys", then pricing-switch-jobs/stage with {"generation":<N>}
```

## Phase D — release advance (manual, engine Control API)

The engine release-v2 resolver admits a model only when the release-pinned catalog covers it,
so after the catalog heads move the release pair itself must advance. The commerce firing pins
for this are gone (see the note above); the remaining path is manual: the kept Stage 5
pair-preparation CLI builds the pair, and the engine Control API
(`http://127.0.0.1:8790`, header `x-api-key: $CONTROL_KEY`, contract in
`docs/engine/CONTROL_API.md`) captures the evidence and performs the head CAS.

1. Prepare the dormant target/recovery release pair with the kept materializer CLI (run from a
   checkout of the exact deployed SHA, with the commerce `DATABASE_URL` and the engine
   credentials in the environment; `OPENKEYS_INTERNAL_BASE_URL`/`OPENKEYS_CONTROL_KEY` default
   to the loopback OpenKeys origin and the engine key):

   ```bash
   # dry-run: no writes; prints the exact plan with its plan_digest and any blockers
   DATABASE_URL=... ENGINE_BASE_URL=http://127.0.0.1:8790 ENGINE_CONTROL_KEY=$CONTROL_KEY \
     pnpm --filter @claude-api/db run pricing:stage5-v2 dry_run
   # apply: materialize only the exact reviewed blocker-free plan
   DATABASE_URL=... ENGINE_BASE_URL=http://127.0.0.1:8790 ENGINE_CONTROL_KEY=$CONTROL_KEY \
     pnpm --filter @claude-api/db run pricing:stage5-v2 apply sha256:v2:<plan_digest>
   ```

   Never materialize a plan with unresolved blockers, and never edit the produced digests.
   Verify the prepared pair:
   `curl -H "x-api-key: $CONTROL_KEY" http://127.0.0.1:8790/admin/pricing/v2/release/<target_generation>`.
2. Capture the full-inventory engine evidence (read-only) for the exact fresh pair with a
   closed window by engine time:

   ```bash
   curl -X POST -H "x-api-key: $CONTROL_KEY" -H 'content-type: application/json' \
     -d '{"target_generation":<N>,"recovery_generation":<N+1>,
          "window_start_ts":<ts>,"window_end_ts":<ts>,
          "min_samples_per_provider":8,"financial_sample_size":100,
          "gemini_client_admissions":<bounded audit count>}' \
     http://127.0.0.1:8790/admin/pricing/v2/stage8-evidence/capture
   ```

   The report must say `passed=true`; `passed=false` is terminal evidence — read the blockers,
   fix the producer, re-capture. The report carries every field the activation evidence needs,
   including the canonical `evidence_digest`.
3. Advance the head immediately (the evidence TTL is 300 s). Build the request from the fresh
   capture report — `target_*`/`recovery_*` from `release`, the inventory/funding/shadow/runtime
   digests and `legacy_inflight_count` verbatim, `engine_captured_ts` = `captured_ts`,
   `observed_ts` = now, `valid_until_ts` = now+300 — and set the expectation to the exact
   current head from `GET /admin/pricing/v2/head`:

   ```bash
   curl -X POST -H "x-api-key: $CONTROL_KEY" -H 'content-type: application/json' \
     -d '{"activation_kind":"successor",
          "expectation":{"exact":{...current head...}},
          "evidence":{...fields from the capture report...},
          "operator_id":"<actor>","reason":"release <model> generation <N>"}' \
     http://127.0.0.1:8790/admin/pricing/v2/activate
   ```

   At the CAS the engine independently recomputes the live inventory, funding manifest and
   runtime floor and rejects any drift with a typed code (`evidence_stale`, `inventory_drift`,
   `funding_drift`, `runtime_floor_drift`, `cas_mismatch`, ...). A rejection is terminal: fix
   the cause and start again from a fresh pair or a fresh capture — never weaken a check, edit
   a durable row, or recompile the B2C policy by hand. That manual recompile hazard is exactly
   why the orchestrated cycle was deleted; do not recreate it as a script. An exact replay of a
   committed activation returns `unchanged`; verify the advance with
   `GET /admin/pricing/v2/head` and the post-activation smoke in Phase E.

## Phase E — publication

Only after the advance is live and proven:

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
no edits of existing migrations or fenced operator state. If a step feels like it needs one of
those, the correct move is a producer fix and a fresh capture, not a workaround.
