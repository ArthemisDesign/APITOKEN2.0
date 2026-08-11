# CHANGE_CHECKLISTS.md — checklists for cross-functional changes

Typical changes that cannot be made in a single place: each one has mirrors and dependent
areas in other contexts. The map of the relationships themselves is `docs/DEPENDENCIES.md`;
the rules for changing contracts are the "Contracts between contexts" section of the root
`AGENTS.md`.

**How to use.** If your change falls under one of the types below — walk the checklist
IN FULL before committing, and state in the commit body which checklist was applied (for
example: "Checklist: new model — all items completed" or "OpenKeys item not applicable,
model is Anthropic-only"). An item must never be silently skipped: it is either completed
or explicitly marked not applicable with a reason. The checklist is a floor, not a
ceiling: if `docs/DEPENDENCIES.md` shows the change has additional consumers, they belong
in the diff or in the report too.

A mechanical formatter-only diff does not fall under these cross-functional checklists: it changes
neither behavior nor a contract, even when `rustfmt` touches a path used by the documentation gate.
Record that determination in the commit body and still run the repository-wide format, build, and
relevant test checks.

## New model (in an existing provider)

Publication is two-stage. The implementation/research merge lands first and stays dormant:
at this step it is forbidden to add the model to production defaults/systemd, the public
model catalog, router presets, the website, or public docs. After GREEN on the exact
implementation SHA, a controlled production live run is performed on an owned credential.
A free authoritative preflight goes first, then a minimal generation. For text/provider surfaces this
is normally `countTokens`. The private Codex image canary instead has a free profile OAuth/quota
preflight through `/wham/usage`; that is not image `countTokens`, tokenization, reserve proof, or
generation evidence. Any paid call above the default aggregate `$0.0001` (0.01 of a cent) cap still
requires explicit authorization for the concrete larger numeric budget and a conservative estimate.
A quota/catalog row, free profile preflight, or `countTokens` does not prove generation. The
publication gate requires simultaneously: generation 2xx, real output, terminal authoritative usage,
incremental SSE, and every
claimed control. Only after that does a separate publication commit walk the public half
of the checklist. Any failed generation means withdrawal: public surfaces are taken down,
and immutable/dormant artifacts are not rewritten.

- [ ] Research/implementation commit: official model/price/control contract, exact private wire
      mapping, and a controlled canary path; runtime implementation is dormant by default.
- [ ] `crates/metering/src/{lib,<provider>}.rs` (including `openai_image.rs`) — tariff table
      (price authority, nanoUSD).
- [ ] `packages/contracts` — public/display model identities such as
      `OPENKEYS_SUPPORTED_MODELS`, only where the publication commit exposes the model.
- [ ] GREEN exact implementation SHA + live gate: generation/output/usage/SSE/controls confirmed
      on every claimed subscription plan/model tier; sanitized evidence recorded in the provider doc.
- [ ] Publication commit is not mixed with the implementation commit; on failed live, a
      withdrawal from every mistakenly touched public/default surface was performed instead.
- [ ] `apps/web/src/lib/models.ts` — SEO catalog (the file header requires synchronization with
      `crates/metering`); done only in the publication commit.
- [ ] `apps/web/src/app/docs/` — docs portal: `integration-builder-data.ts` and, if the model
      appears in the reference, `api-reference-data.ts` / `docs-portal.tsx`; only after the live gate.
- [ ] Production defaults/systemd and `crates/router/routing-presets.json` — only after the live gate.
- [ ] Discovery: the serving plane's `GET /v1/models{,/{id}}` lists the model AND that plane's
      `/internal/router/catalog/pricing` producer resolves a rate card for it. The router drops
      every catalog entry it cannot price, so either half missing leaves the model invisible to
      catalog-driven clients while its routes work (this is how gpt-image-2 shipped). A model that
      the text lanes cannot run is rejected there with a `400` naming its real endpoint, not a `404`.
- [ ] `crates/metering` carries an exact tariff for the model BEFORE it is advertised. The gate
      `advertised_models_all_have_an_exact_tariff` (`crates/router/src/tests.rs`) fails the build
      otherwise, and a dated snapshot id additionally needs an explicit entry in
      `ANTHROPIC_DATED_SNAPSHOT_ALIASES` — the alias table stays deliberate so an unpublished
      future snapshot can never inherit today's price.
- [ ] `docs/engine/<provider>.md` — provider's model list.
- [ ] `docs/ops/MODEL_RELEASE_CYCLE.md` — no capability/policy/catalog generation or release-head
      advance. Verify the compiled tariff and bridge it into the hot tariff table with
      `POST /admin/pricing/tariffs/seed`; later corrections use `.../override`.
- [ ] `apps/openkeys` — every runtime-priceable model remains available at the fixed 1:1 account
      scalar; after the live gate update `OPENKEYS_SUPPORTED_MODELS` only if the issuance/display
      guidance should name the model, and prove no OpenKeys-specific allowlist blocks execution.
- [ ] `docs/commerce/PRICING.md` — if the model changes customer pricing.
- [ ] `apps/admin` — if the model is visible in quotas/calculator (`subscriptions`,
      `sales/calculator/calculation.ts`).

## Price or multiplier change

- [ ] Classify the change first: an official provider tariff belongs to `crates/metering` plus the
      hot tariff authority; a customer payable multiplier belongs to the account default or one
      canonical provider override. Do not encode either in the retired policy/release schema.
- [ ] Account multiplier changes are persisted through the atomic commerce writer and
      `engine_pricing_jobs`, never one-off engine calls. Walk `packages/db/src/pricing.ts`,
      `packages/db/src/pricing-discounts.ts`, `apps/worker/src/pricing-worker.service.ts`,
      `packages/engine-client` and the engine Control API default/provider writes. Terminal queue
      writes must retain exact lease fencing and stale-desired requeue.
- [ ] `packages/contracts` and both PostgreSQL schemas keep the same closed multiplier range
      `0..10000` and provider set `anthropic|openai|google|kimi|glm`; no model-specific customer
      multiplier schema exists.
- [ ] `apps/web` — storefront figures: remove dependencies on `src/lib/pricing-tiers.ts`, check
      `src/lib/models.ts` and all
      storefront copy (`src/components/marketing-pages.tsx`, `src/components/cost-calculator.tsx`,
      `src/lib/md-pages.ts`, `src/lib/messages.json`, `src/lib/llms.ts`, `src/lib/learn-*.ts`).
      Determine the radius by grepping for the old figure, not from memory.
- [ ] `docs/commerce/PRICING_MODEL.md` and the `PRICING.md` entry point — account default/provider
      pricing, B2C/B2B/OpenKeys/service and paid/free attribution.
- [ ] Prices are hot data: publish the new family version through
      `POST /admin/pricing/tariffs/override` (or `.../seed`) on the engine Control API
      (`docs/engine/CONTROL_API.md`) — the deleted Stage 5–9 release cycle, its orchestrator,
      and its gates must not be revived for a price change. Customer discounts use only the fenced
      scalar/provider delivery queue.
- [ ] B2B keeps its negotiated default plus explicit provider overrides; OpenKeys stays 1:1;
      service stays zero-payable and metered. Explicitly mark the classes that do not apply.
- [ ] Partner calculations: `docs/sales/SALES_PAYOUT_PERIODS.md`, `apps/sales-api` logic —
      if the price feeds the payout base.
- [ ] Sales feed/commission: the live scalar producer emits collected customer-funded
      `real_funded_nano`; welcome/promo/admin credit and settlement shortfall are excluded. If the
      wire schema changes, walk the separate sales feed checklist.
- [ ] `apps/admin/src/app/sales/calculator/calculation.ts` — `PRODUCT_CATALOG`.

## New subscription provider

Full order research → credential/Auth Bot → runtime → money/calibration → admin → blue-green →
live GA: `docs/engine/PROVIDER_ONBOARDING.md`. The checklist below is an index of the mandatory
radius, not a replacement for the phase gates and Definition of GA from the playbook.

- [ ] Credential crate `crates/<provider>-credential` (encrypted OAuth envelopes, no network)
      + `crates/<name>/CLAUDE.md` per the "living contract" rules.
- [ ] `crates/metering/src/<provider>.rs` — tariff table.
- [ ] Engine runtime: provider transport/pool/billing in `crates/forward` (the bulk of the code),
      mode in `crates/server`, slots/ports in `deploy/Caddyfile`, systemd units.
- [ ] Pool replenishment: OAuth provisioning in `crates/authbot` (if the provider is subscription-based).
- [ ] Sticky/unlimited-parallel runtime: no local queue/semaphore/reject; retry only before
      the first public byte; disconnect drain preserves terminal usage and settlement.
- [ ] Durable reserve/delivering/settlement + exact immutable turn evidence; official API nanoUSD and
      native subscription credits are tracked separately.
- [ ] Calibration of every native window per the Codex fixed-point/raw-evidence contract; exact plan +
      duration cohorts, null until evidence, no nominal/prior/EMA.
- [ ] Exact turn delivery has bounded FIFO, idempotent replay, conflict quarantine, pending/drop
      diagnostics, and shutdown drain; a quota snapshot can never overtake a failed spend event.
- [ ] Delivery: `config.env.example`, deploy scripts (`watchdog.sh`, `watchdog-lib*.sh`,
      `engine-bluegreen.sh`, `sudoers.d`) — the provider's new ports, units, and secrets.
- [ ] Observability: metrics and alerts in `observability/prometheus/rules/*`, Grafana dashboards,
      runbook sections in `docs/ops/MONITORING.md` (see the "New alert or metric" checklist).
- [ ] `docs/engine/<PROVIDER>_PROVIDER.md` — new document + a line in `docs/README.md`.
- [ ] `docs/commerce/PRICING_MODEL.md` plus engine/commerce schemas — add the canonical provider ID
      to both closed provider CHECKs and both write validators if customers may receive a provider
      override; never infer the ID from a model or revive a provider-switch policy.
- [ ] `packages/contracts` — public/display model identities and strict wire schemas that actually
      cross a live boundary.
- [ ] `apps/web` (storefront), `apps/openkeys`, `apps/admin` — display and sale.
- [ ] `apps/admin` adds the provider to the single compact fleet control-room and one account-table:
      real provider windows, exact remaining/full API-$, used rail, readiness/coverage, and masked
      identity. Raw token/profitability/cache/quota-bucket matrices stay backend/report evidence,
      and pending/degraded authority hides saleable money instead of showing stale `$`.
- [ ] Controlled live matrix of every published plan/model/tier + public post-deploy smoke;
      the exact landed SHA has `deploy/watchdog` GREEN.
- [ ] A line in `docs/DEPENDENCIES.md`.

## Control API change (engine ↔ commerce/OpenKeys)

The contract is expand-only — see the protocol in `AGENTS.md`. Order: the producer (engine)
first, consumers after a green `deploy/watchdog` on the producer SHA.

- [ ] `crates/server/src/http.rs` + `src/admin.rs` — routes/handlers.
- [ ] `docs/engine/CONTROL_API.md` — IN THE SAME commit as the engine code.
- [ ] `packages/contracts` — zod schemas for new/extended messages.
- [ ] `packages/engine-client` — client methods (a separate step after the engine deploy).
- [ ] Consumers per `docs/DEPENDENCIES.md`: `apps/api`, `apps/worker`, `apps/openkeys`
      (after the engine deploy). `apps/admin` goes through the Caddy proxy — verify the
      operator routes separately.

## Sales feed change (commerce ↔ sales)

The contract is expand-only; the types are duplicated as local zod schemas on BOTH sides —
both are edited.

- [ ] Producer `apps/api/src/sales-feed.controller.ts` (or `apps/sales-api/src/internal.controller.ts`
      for the reverse direction) — first, per the contract protocol.
- [ ] Consumer `apps/sales-api` (`sync.service.ts`, `commerce.service.ts`) or `apps/api`
      (`promo.service.ts`, `auth.service.ts`) — after the producer deploy.
- [ ] `apps/sales-web` — partner frontend (`referrals`, `partner-analytics`, `lib/api.ts`),
      if the feed change is visible to the partner.
- [ ] `docs/sales/SALES_PORTAL.md` — the "The sales ↔ commerce boundary" section in the same commit.
- [ ] The sales feed line in `docs/DEPENDENCIES.md` — if the set of endpoints changes.
- [ ] Update `tests/contracts/sales-usage-feed.golden.json` and keep producer serialization plus
      consumer parsing pinned to that same row. Add an end-to-end rejection/acceptance case for any
      new nullable combination; isolated producer and consumer unit tests are insufficient.

## New payment method

- [ ] `packages/payments/src/<provider>.ts` — adapter + registration in the registry
      (`apps/api/src/payments.module.ts`, env factory).
- [ ] `PaymentProviderCode` and checkout schemas: `apps/api/src/checkout.service.ts`,
      `packages/contracts`.
- [ ] Webhook in `apps/api/src/payments.controller.ts` (+ exclusion from the origin guard) and/or
      reconcile polling in `apps/worker`.
- [ ] `docs/commerce/<PROVIDER>_INTEGRATION.md` — new document + a line in `docs/README.md`.
- [ ] `apps/web` — provider selection at checkout; `apps/admin` — finance storefronts.
- [ ] A line in `docs/DEPENDENCIES.md`.

## New alert or metric

- [ ] `observability/prometheus/rules/{application,operations}.yml` — alert with the annotation
      `runbook: 'docs/ops/MONITORING.md#<alert>'`.
- [ ] `docs/ops/MONITORING.md` — a `## <Alert>` section IN THE SAME commit (without it
      `deploy/monitoring-config.test.sh` will not pass).
- [ ] If the metric is new — the collector must export it (checked by the same script).
