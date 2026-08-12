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
| Metering tariff | (this commit) | `crates/metering/src/tripo3d.rs`; official per-task rate card §5.1, fail-closed catalog §3, checked nanoUSD at $0.01/credit; 16 exact-vector tests |
| Migration 0049 + registry observation types | (this commit) | `crates/registry/migrations_pg/0049_tripo3d_calibration.sql` + `crates/registry/src/tripo3d_calibration.rs`; windowless dual-ledger authority per manifest §5.3 (millicredits + fixed-rate nanoUSD legs, verbatim raw balance halves with NULL parsed units, subject+cohort state, cold/measured CHECK); `tripo3d_*` PG family mirrors `glm_*`; real-PG replay/CAS matrix green on a scratch DB |
| Pricing provider set (migration 0051) | (this commit) | `crates/registry/migrations_pg/0051_tripo3d_pricing_provider.sql` (schema 50 → 51): the runtime reserves under `provider = 'tripo3d'`, so both closed sets widen expand-only — `account_provider_discounts_provider_id_check` (0046) and `reservations_scalar_pricing_shape` (0047), drop + re-add strictly wider NOT VALID + VALIDATE; `DISCOUNT_PROVIDER_IDS` 5→6 and the legacy SQLite CHECK texts mirror it; new registration/content tests, the three `CURRENT_SCHEMA_VERSION` pins move 50→51 |
| Credential crate | (this commit) | `crates/tripo3d-credential`; GLM-pattern AEAD envelope for the static `tsk_` key + declared top-up cohort (lowercase-normalized, matches 0049 `cohort`); two-origin base-URL allowlist (global/CN) with the `test-loopback-base-url` escape; no rotate surface; 19 tests green with and without the feature |
| Calibration estimator | (this commit) | `crates/forward/src/tripo3d_calibration.rs`; windowless balance-track state machine per manifest §5.3 (anchor/cold branch while the unit is unproven, top-up re-anchor, quota-before-settlement hold, repeated balance-only movement excluded via anchor advance — the 0049 state schema has no unattributed column, version rebuild from immutable history, checked i64/i128 only); capacity = remaining × ΣΔapi/ΣΔbalance drawdown with the quantisation envelope from the raw decimal resolution, high stays `None` when unproven; 28 deterministic tests |
| Auth Bot protocol + roster | (this commit) | `crates/authbot/src/{tripo3d_key,tripo3d_roster}.rs` + `main.rs` env (`AUTH_BOT_TRIPO3D_{DIR,CREDENTIAL_KEYS,CREDENTIAL_ACTIVE_KID}`) + `tripo3d-credential` dep; GLM-pattern static-key intake per manifest §7: free Bearer balance probe (HTTP 401 / body code 401 → invalid key, other business codes transport-class, raw `balance`/`frozen` tokens), fail-closed cohort corroboration (probe succeeded + both counters parse as non-negative decimals — no magnitude comparison while the unit is unproven, §5.2), **no paid admission task at all** ($0.05 cheapest task > $0.0001 cap, §7 open question), `tcli_` Client ID refused locally; roster = glm_roster mirror with key-as-identity replace-in-place; modules dormant (`#![allow(dead_code)]`) until the wizard commit; 17 new tests |
| Auth Bot seller wizard | (this commit) | `HandoffKind::Tripo3d` in `crates/authbot/src/bot.rs` per manifest §7: steps `t3_proxy → t3_ready → t3_wait`, button `t3:ready`, platform sub-buttons `t3:region:global\|cn` (choice persisted in the shared `users.hregion`, reset per deal, global default); tier codes `tripo3d_api_{25,50,100}` ("Tripo3D API $25/$50/$100") across `product_kb`/`tier_name`/`admin_quick_tier`/`admin_home_kb`; classification keys on `tripo3d`/`tripo` above the Claude fallback; seller texts (offer guide, account setup, proxy prompt, key prompt + typed static guidance incl. `tcli_` misuse); full GLM-mirror wizard (`prepare_tripo3d_account`, readiness gate `t3:ready` requires step+stored proxy, `handle_tripo3d_key_message`: preflight → bounded free balance probe → fail-closed cohort corroboration → seal → atomic publish → payout, keyring-gated dormant intake, key never logged/echoed/persisted); step-back edges incl. the confirm + degrade-to-proxy rule; `db.rs` `recover_interrupted_tripo3d_handoffs` (`t3_wait`→`t3_ready` on restart) wired in `main.rs`; batch/buyer/IPRoyal start paths extended; 21 new tests, exhaustive menu/step-back tests extended (hard counter 15→18) |
| Runtime primitives | (this commit) | `crates/forward/src/tripo3d/{mod,config,transport,roster,client,selection,pool,queue}.rs` + `ProviderMode::Tripo3d` in `state.rs` (+ the two exhaustive matches: `metrics.rs` plane series, `http.rs` router placeholder 404 arm until the server wiring commit); GLM-pattern plane but task-based: money boundary = successful upstream `code:0 + task_id` create (pool.rs `Phase`), hard axis = provider verdicts only (429+`2000`+`Retry-After`, 403+`2010`, proven balance shortfall), soft axis = 401/transport/probe (never empties the pool — `select_ignoring_soft`), balance probe preserves raw decimal text (never float), `consumed_credit` → strict millicredits (≤3 decimals, fail closed finer), artifact field allowlist `{model,base_model,pbr_model,rendered_image}`, turn FIFO gates balance polls; modules dormant (`#![allow(dead_code)]`) until the gateway commit; 67 new tests |

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

- Resolved metering ambiguities now baked into the tariff: P1 rejects surcharges (all-in);
  `highpoly_to_lowpoly` fails closed on both conflicted version spellings; `generate_image`
  priced at the conservative upper bound 10; mesh-completion `P-v2.0-20251225` fails closed.
- Everything downstream of the tariff (see Queue).

## Next action (exactly one)

Task-lifecycle gateway + durable billing (`crates/forward/src/tripo3d/gateway.rs` +
`crates/forward/src/billing.rs` Tripo3D writer commands, separate commit): reserve → select →
create → delivering → detached poll (2s cadence, bounded deadline) → immediate artifact download
into `ARTIFACT_DIR` (≤60s upstream URL TTL) → settle exact `consumed_credit` → turn FIFO →
registry authority → balance observations. Then server wiring (`crates/server`). The Auth Bot
branch is delivered and dormant on the `AUTH_BOT_TRIPO3D_*` keyring; runtime primitives are
delivered and dormant under `ProviderMode::Tripo3d`.

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
