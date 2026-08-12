# Suno plane progress — resumable ledger

Terminal state: an operator acquires a Suno subscription through Auth Bot, the resulting profile
becomes routable capacity in the engine, traffic is billed against the official Suno rate card
(integer nanoUSD), calibration records evidence from real quota movement, and the provider appears
in the admin control room. Publication (production defaults, public catalog, router presets,
storefront, public docs) is OUT of scope for this task — dormant implementation only.

## Done

| Step | Commit SHA | Notes |
|---|---|---|
| Ledger skeleton | e9b0494e | scope, plan, open seams |
| Pre-flight + wiring maps | 1e15c793 | baseline `cargo build --locked` green; push works; engine + authbot/admin wiring maps collected (KIMI/GLM templates). See TRIPO3D_PLANE_PROGRESS.md "Key wiring facts" — shared with this plane |
| Research + capability manifest | (this commit) | `docs/engine/SUNO_PROVIDER.md`; official pricing/ToS/blog + OSS wire blueprint research 2026-08-12; key facts below |
| Metering tariff | (this commit) | `crates/metering/src/suno.rs`; reviewed derived schedule §5.1 ($0.004/credit, 5 cr/song = $0.02), fail-closed paid model catalog; 7 exact-vector tests |

## Key research facts (review date 2026-08-12)

- **No public official API** (`platform.suno.com` is partner-gated). Only path: session-cookie
  pooling on internal web endpoints (`auth.suno.com` Clerk `__client` → JWT mint;
  `studio-api.prod.suno.com` business host) — gcui-art/suno-api blueprint, `oss-hypothesis`.
- Plans: Free (50 cr/day, `v4.5-all`, excluded) / Pro $10 (2 500 cr/mo) / Premier $30
  (10 000 cr/mo); paid models v4/v4.5/v4.5+/v5/v5.5; **all models retire** when partnership
  models launch (new ToS 2026-09-03).
- Money: 5 credits/song (only published per-op price for generation); no official API rate card →
  reviewed derived schedule $0.004/credit (Pro economics, conservative) = $0.02/song.
- Quota: `/api/billing/info/` → `total_credits_left/monthly_limit/monthly_usage` (oss); monthly
  window, published per-plan limits (GLM-style, no capacity estimation needed).
- ToS prohibits resale/automation/scraping/proxy-circumvention → backend-only, dormant.
- hCaptcha may gate generation (`/api/c/check`); no CAPTCHA solving is built — fail closed.
- Serving: own ProviderMode plane (blue-green pair), create→poll→download→settle.
- Open admission-budget question: one admission song = 5 credits = $0.02 derived > default
  $0.0001 cap — needs explicit operator budget or admission stops at the free probe.

## Open seams

- The schedule is DERIVED, not official: a top-up price list or an official API rate card, once
  available, replaces it as a new dated epoch (`suno/derived-subscription/...` → new id; the
  `suno/credits` override family stays stable). Wire spellings of the model ids remain `unknown`.
- Everything downstream of the tariff (see Queue).

## Next action (exactly one)

Migration `crates/registry/migrations_pg/0050_suno_window_calibration.sql` (expand-only,
separate commit) — monthly-window calibration schema sized to the manifest §5.2/§5.3 quota
facts (published per-plan limits 2 500 / 10 000 credits, GLM-style, no capacity estimation).

## Queue

1. Research + capability manifest (`docs/engine/SUNO_PROVIDER.md` draft, evidence-labeled).
2. Metering tariff `crates/metering/src/suno.rs`.
3. Migration (expand-only, separate commit) — window calibration schema sized to real quota facts.
4. Credential crate `crates/suno-credential`.
5. Estimator `crates/forward/src/suno_calibration.rs`.
6. Auth Bot protocol + wizard (`crates/authbot`).
7. Runtime transport/pool/billing (`crates/forward`), server wiring (`crates/server`).
8. Admin control room (`apps/admin`).
9. Observability/deploy surfaces (dormant: no production ports/units enabled).
10. Live matrix — BLOCKED on a human-owned live subscription.

## Blocked on human

- Live Suno subscription for the GA live gate (calibration runner, controlled matrix).
