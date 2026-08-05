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
The free `countTokens` goes first, then a minimal generation; a provider surface without
`countTokens` (such as Images) cannot silently skip this step and requires a separate reviewed,
image-specific free-preflight admission exception before any paid attempt. The default aggregate
admission budget is no more than `$0.0001` (0.01 of a cent). A quota/catalog row and
`countTokens` do not prove generation. The publication gate requires simultaneously:
generation 2xx, real output, terminal authoritative usage, incremental SSE, and every
claimed control. Only after that does a separate publication commit walk the public half
of the checklist. Any failed generation means withdrawal: public surfaces are taken down,
and immutable/dormant artifacts are not rewritten.

- [ ] Research/implementation commit: official model/price/control contract, exact private wire
      mapping, and a controlled canary path; runtime implementation is dormant by default.
- [ ] `crates/metering/src/{lib,<provider>}.rs` (including `openai_image.rs`) — tariff table
      (price authority, nanoUSD).
- [ ] `packages/contracts` — `CURRENT_*_CANONICAL_MODELS` and/or pricing schemas.
- [ ] GREEN exact implementation SHA + live gate: generation/output/usage/SSE/controls confirmed
      on every claimed subscription plan/model tier; sanitized evidence recorded in the provider doc.
- [ ] Publication commit is not mixed with the implementation commit; on failed live, a
      withdrawal from every mistakenly touched public/default surface was performed instead.
- [ ] `apps/web/src/lib/models.ts` — SEO catalog (the file header requires synchronization with
      `crates/metering`); done only in the publication commit.
- [ ] `apps/web/src/app/docs/` — docs portal: `integration-builder-data.ts` and, if the model
      appears in the reference, `api-reference-data.ts` / `docs-portal.tsx`; only after the live gate.
- [ ] Production defaults/systemd and `crates/router/routing-presets.json` — only after the live gate.
- [ ] `docs/engine/<provider>.md` — provider's model list.
- [ ] `docs/commerce/MULTI-DISCOUNT.md` §7 — a new model is NOT included automatically:
      an explicit catalog generation is required (catalogs/switches/policies in
      `crates/registry/src/pricing/` via the versioned pricing protocol of the Control API).
- [ ] `apps/openkeys` — `assertOpenKeysCatalog()` is checked against `CURRENT_PRODUCT_CATALOG_ENTRIES`
      from `packages/contracts`: without a catalog update OpenKeys fails closed.
- [ ] `docs/commerce/PRICING.md` — if the model changes customer pricing.
- [ ] `apps/admin` — if the model is visible in quotas/calculator (`subscriptions`,
      `sales/calculator/calculation.ts`).

## Price or multiplier change

- [ ] `crates/metering` — authority table (official provider prices), reviewable commit.
- [ ] Engine multiplier changes only through durable jobs — never one-off calls (invariant from
      `CLAUDE.md`); the protocol is versioned pricing in `docs/engine/CONTROL_API.md`. Machinery:
      `packages/db/src/pricing-control-jobs.ts`, `apps/worker/src/pricing-worker.service.ts`,
      `packages/engine-client` (ledger/ack), execution in `crates/forward/src/**/billing.rs`.
- [ ] `packages/contracts` — global/provider/model discount and pricing release schemas;
      `packages/db/src/pricing.ts`. Do not keep `B2C_PRICING_TIERS` as a target authority.
- [ ] `apps/web` — storefront figures: remove dependencies on `src/lib/pricing-tiers.ts`, check
      `src/lib/models.ts` and all
      storefront copy (`src/components/marketing-pages.tsx`, `src/components/cost-calculator.tsx`,
      `src/lib/md-pages.ts`, `src/lib/messages.json`, `src/lib/llms.ts`, `src/lib/learn-*.ts`).
      Determine the radius by grepping for the old figure, not from memory.
- [ ] `docs/commerce/PRICING.md` — global/provider/model pricing, B2B/OpenKeys/service, and bonus.
- [ ] `docs/commerce/MULTI-DISCOUNT.md` + Stage 5/6/8/9 — target/recovery release, full inventory,
      100% shadow, and one-head activation; do not add per-account canary/maintenance rollout.
- [ ] Production Stage 5/6 runs only through the protected AdminGuard producer API: verified actor,
      fresh exact plan digest, meaningful mutation reason, attributed audit, and strict status;
      package CLI/manual SSH does not count as an operator surface. Wire the UI consumer in a
      separate commit only after GREEN producer SHA.
- [ ] A terminal pre-cutover `strict + legacy_single` delivery is recovered only through exact-CAS
      `/v1/admin/pricing-policy-delivery-repairs`: never rewrite the old payload, never retry a
      generic dead job, and never fix commerce rows by hand.
- [ ] B2B current discount stays an independent Anthropic rule; OpenKeys stays 1:1; service
      stays `meter_only` and all-model. Explicitly mark the classes that do not apply.
- [ ] Partner calculations: `docs/sales/SALES_PAYOUT_PERIODS.md`, `apps/sales-api` logic —
      if the price feeds the payout base.
- [ ] Sales feed/commission: exact `paid_funded_nano` must not depend on pricing mode; the welcome
      bonus is excluded. If the wire schema changes, walk the separate sales feed checklist.
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
- [ ] `docs/commerce/MULTI-DISCOUNT.md` — catalogs, provider switch (§8), policies.
- [ ] `packages/contracts` — canonical models, product catalogs.
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
