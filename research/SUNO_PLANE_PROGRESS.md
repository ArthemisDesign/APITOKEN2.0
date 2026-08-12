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
| Migration 0050 + registry observation types | (this commit) | `crates/registry/migrations_pg/0050_suno_window_calibration.sql` + `crates/registry/src/suno_calibration.rs`; GLM-style monthly-window authority per manifest §5.2/§5.3 (millicredits + derived fixed-rate nanoUSD legs with the `native_schedule_derived` flag, verbatim raw quota counters with NULL derived fields, exact-duration window keying with no synthetic constant, Pro/Premier-only CHECK, cold/measured split); `suno_*` PG family mirrors `glm_*`; real-PG replay/CAS matrix green on a scratch DB |
| Migration 0052 (pricing provider admission) | (this commit) | `crates/registry/migrations_pg/0052_suno_pricing_provider.sql` widens the two closed provider CHECK sets (`account_provider_discounts_provider_id_check`, `reservations_scalar_pricing_shape`) with `'suno'` exactly as 0051 did for tripo3d (drop/re-add NOT VALID + VALIDATE, `engine_schema_migrations` 52); `DISCOUNT_PROVIDER_IDS` 6→7, SQLite mirror texts, `CURRENT_SCHEMA_VERSION` 51→52 with registration/content tests; lands alone before any code reserving under `provider = 'suno'` |
| Credential crate | (this commit) | `crates/suno-credential`; GLM-pattern AEAD envelope for the Clerk session cookie (`__client=` entry enforced at seal) + optional rediscoverable session id + declared plan `Pro`/`Premier` (labels match the 0050 CHECK; `SUNO_REVIEWED_PLANS` pins 2 500/10 000 credits, reviewed 2026-08-12); fixed-host constants, no base-url override by design; JWT mint / `set-cookie` re-seal single-flight documented as the runtime's concern; 18 tests green |
| Calibration estimator | (this commit) | `crates/forward/src/suno_calibration.rs`; GLM dual-path monthly-window state machine per manifest §5.2/§5.3 (quota-endpoint fraction when carried, else native-ledger fraction against `suno_credential::reviewed_plan_credits`; cutover re-anchors without erasing history, exact-duration keying, unattributed counter, version rebuild from immutable history, checked i64/i128 only); §10.6 cohort pooling by exact plan+duration; 33 deterministic tests |
| Auth Bot protocol + roster | (this commit) | `crates/authbot/src/{suno_session,suno_roster}.rs` + `main.rs` env (`AUTH_BOT_SUNO_{DIR,CREDENTIAL_KEYS,CREDENTIAL_ACTIVE_KID}`) + `suno-credential` dep; GLM-pattern intake per manifest §7 over the `__client` cookie artifact (sanctioned one-time artifact, manifest §2 deviation): Clerk session discovery → JWT mint → free billing probe (401/403 → typed Auth verdict, any schema deviation fails closed, raw nullable counters), plan corroboration against the published monthly ladder (2 500/10 000 exact match, mismatch/unreadable fail closed), **no paid admission song at all** ($0.02 > $0.0001 cap, §7 open question); roster = glm_roster mirror with session-id-as-identity replace-in-place (no session id → publication refused); modules dormant (`#![allow(dead_code)]`) until the wizard commit; 24 new tests |
| Auth Bot seller wizard | (this commit) | `HandoffKind::Suno` in `crates/authbot/src/bot.rs` per manifest §7: steps `su_proxy → su_ready → su_wait`, button `su:ready`, no region row (single platform); tier codes `suno_pro`/`suno_premier` ("Suno Pro"/"Suno Premier") across `product_kb`/`tier_name`/`admin_quick_tier`/`admin_home_kb`; classification keys on `suno` above the Claude fallback (bare "Pro"/"Premier" never classify); seller texts walk proxy-first setup → exact plan activation → guided `__client` copy from the seller's own browser (sanctioned one-time artifact); full GLM-mirror wizard (`prepare_suno_account`, readiness gate `su:ready` requires step+stored proxy, `handle_suno_cookie_message`: shape preflight → discovery → JWT mint → billing probe (bounded retry each) → plan corroboration → seal → atomic publish → payout, keyring-gated dormant intake, cookie never logged/echoed/persisted); step-back edges incl. the confirm + degrade-to-proxy rule; `db.rs` `recover_interrupted_suno_handoffs` (`su_wait`→`su_ready` on restart) wired in `main.rs`; batch/buyer/IPRoyal start paths extended; 20 new tests, exhaustive menu/step-back tests extended (hard counter 18→20) |
| Migration 0052 (pricing provider admission) | (this commit) | `crates/registry/migrations_pg/0052_suno_pricing_provider.sql` widens the two closed provider CHECK sets (`account_provider_discounts_provider_id_check`, `reservations_scalar_pricing_shape`) with `'suno'` exactly as 0051 did for tripo3d (drop/re-add NOT VALID + VALIDATE, `engine_schema_migrations` 52); `DISCOUNT_PROVIDER_IDS` 6→7, SQLite mirror texts, `CURRENT_SCHEMA_VERSION` 51→52 with registration/content tests; lands alone before any code reserving under `provider = 'suno'` |

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

Runtime transport/pool/billing (`crates/forward/src/suno/**`, separate commit) — per the
queue below: config/transport/roster/pool/selection/queue/client + the create → poll →
download → settle gateway on the fixed-host contract (with the JWT-mint / `set-cookie`
re-seal single-flight per manifest §2), then server wiring (`crates/server`). The Auth Bot
branch is delivered and dormant on the `AUTH_BOT_SUNO_*` keyring.

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
