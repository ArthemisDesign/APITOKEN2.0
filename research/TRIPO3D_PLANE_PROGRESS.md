# Tripo3D plane progress — resumable ledger

Terminal state: an operator acquires a Tripo3D subscription/API account through Auth Bot, the
resulting profile becomes routable capacity in the engine, traffic is billed against the official
Tripo3D rate card (integer nanoUSD), calibration records evidence from real quota movement, and the
provider appears in the admin control room. Publication (production defaults, public catalog, router
presets, storefront, public docs) is OUT of scope for this task — dormant implementation only.

## Done

| Step | Commit SHA | Notes |
|---|---|---|
| Ledger skeleton | (this commit) | scope, plan, open seams |

## Open seams

Everything. Research not started.

## Next action (exactly one)

Research Tripo3D: official API surface, model/task catalog, subscription plans, credit/pricing
schedule, authentication mechanism. Record into the capability manifest.

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
