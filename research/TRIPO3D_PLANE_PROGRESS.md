# Tripo3D plane progress — resumable ledger

Terminal state: an operator acquires a Tripo3D subscription/API account through Auth Bot, the
resulting profile becomes routable capacity in the engine, traffic is billed against the official
Tripo3D rate card (integer nanoUSD), calibration records evidence from real quota movement, and the
provider appears in the admin control room. Publication (production defaults, public catalog, router
presets, storefront, public docs) is OUT of scope for this task — dormant implementation only.

## Done

| Step | Commit SHA | Notes |
|---|---|---|
| Ledger skeleton | e9b0494e | scope, plan, open seams |
| Pre-flight + wiring maps | 1e15c793 | baseline `cargo build --locked` green (1m12s, warnings only); `git push -u origin HEAD` works; engine wiring map and authbot/admin wiring map collected (KIMI/GLM as templates); reference manifests studied: KIMI, GLM |
| Research + capability manifest | (this commit) | `docs/engine/TRIPO3D_PROVIDER.md`; full official docs + ToS + SDK research 2026-08-12; key facts below |

## Key research facts (review date 2026-08-12)

- Two billing systems: Studio subscriptions (web) vs API platform (prepaid credits). Provider =
  **API platform only**: `api.tripo3d.ai/v2/openapi` (global) / `api.tripo3d.com` (CN), Bearer
  `tsk_` keys from `platform.tripo3d.ai/api-keys`.
- Money: $0.01/credit prepaid; per-task credit costs official (billing.md); per-turn authoritative
  `consumed_credit`; failed/expired tasks refund. Balance endpoint `/user/balance` is SDK-verified
  only (unit unknown → raw evidence).
- Tasks are per-key isolated; polling-only (no webhooks); result URLs ≤60 s (conservative).
- ToS forbids resale/pooling/multi-account without written consent → backend-only, dormant.
- Serving: own ProviderMode plane (blue-green pair, KIMI deploy pattern), task lifecycle
  create→poll→download-to-our-storage→settle.
- Open admission-budget question: cheapest paid task costs 5 credits = $0.05 > default $0.0001
  admission cap — needs explicit operator budget or a confirmed free probe.

## Key wiring facts (from the mapping pass)

- Template recipe (KIMI/GLM pattern, dependency order): credential crate → metering →
  migration+registry types → forward plane (config/transport/roster/pool/selection/queue/client/
  gateway + estimator) → server (env keys, compose, admin route, metrics) → authbot (roster +
  intake) → observability → deploy → admin UI.
- Closed provider-id set `anthropic|openai|google|kimi|glm` lives in: registry SQLite CHECK
  (`crates/registry/src/lib.rs:401,1070`), PG migrations 0046/0047, commerce
  `packages/db/migrations/0045`, `packages/contracts` zod enum, `packages/db/pricing-discounts.ts`,
  admin-finance filters, router pricing producer (`crates/server/src/router_pricing.rs:123`).
  Extending = expand-only, producer-first.
- KIMI/GLM ride the Anthropic Messages plane via model-alias dispatch in `proxy.rs`; Tripo3D is a
  task-based 3D API and will NOT speak Anthropic Messages — the closest serving template is the
  Codex image lane (`crates/forward/src/codex/images.rs`, `/v1/images/generations` in
  `crates/server/src/http.rs`). Final serving boundary decided after research.
- authbot: `HandoffKind` enum + exhaustive matches in `bot.rs`; menu tests carry a hard button
  counter; new provider = new step-id pair, tier codes, seller texts, readiness handler, roster
  module; intake gated on `AUTH_BOT_<P>_*` keyring env.
- admin: new provider = fetch in `page.tsx loadSubs()`, types block, `logic.ts` helpers, fleet
  card (closed `FleetCardValue.id` union), `<id>-capacity-board.tsx`, CSS accents, Caddy admin
  route, `sources.ts`, tests.

## Open seams

- Metering tariff `crates/metering/src/tripo3d.rs` — next code step.
- Everything downstream of the manifest (see Queue).

## Next action (exactly one)

Write `crates/metering/src/tripo3d.rs` (official per-task credit rate card from manifest §5.1,
nanoUSD conversion at $0.01/credit, checked math, fail-closed catalog) + full metering tests.

## Queue

1. Research + capability manifest (`docs/engine/TRIPO3D_PROVIDER.md` draft, evidence-labeled).
2. Metering tariff `crates/metering/src/tripo3d.rs`.
3. Migration (expand-only, separate commit) — window calibration schema sized to real quota facts.
4. Credential crate `crates/tripo3d-credential`.
5. Estimator `crates/forward/src/tripo3d_calibration.rs`.
6. Auth Bot protocol + wizard (`crates/authbot`).
7. Runtime transport/pool/billing (`crates/forward`), server wiring (`crates/server`).
8. Admin control room (`apps/admin`).
9. Observability/deploy surfaces (dormant: no production ports/units enabled).
10. Live matrix — BLOCKED on a human-owned live subscription.

## Blocked on human

- Live Tripo3D subscription/account for the GA live gate (calibration runner, controlled matrix).
