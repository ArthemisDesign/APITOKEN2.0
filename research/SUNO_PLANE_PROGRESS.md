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
| Migration 0052 (pricing provider admission) | 785c4a00 | `crates/registry/migrations_pg/0052_suno_pricing_provider.sql` widens the two closed provider CHECK sets (`account_provider_discounts_provider_id_check`, `reservations_scalar_pricing_shape`) with `'suno'` exactly as 0051 did for tripo3d (drop/re-add NOT VALID + VALIDATE, `engine_schema_migrations` 52); `DISCOUNT_PROVIDER_IDS` 6→7, SQLite mirror texts, `CURRENT_SCHEMA_VERSION` 51→52 with registration/content tests; lands alone before any code reserving under `provider = 'suno'` |
| Runtime primitives | dc195b8d | `crates/forward/src/suno/{mod,config,transport,roster,client,selection,pool,queue}.rs` on the Tripo3D template with the session differences the provider forces: default-off config (no base-url field — fixed official hosts), HTTP-only conservative classification (429 hard rate wall, 401/403 post-mint soft, no documented business codes), Clerk/billing/CAPTCHA/create/feed/clip/lyrics parsers that fail closed on schema deviation with raw nullable quota counters, `set-cookie` merge primitive for the single-flight, session-id subject digest (`apitoken/suno-subject-identity/v1`), hard/soft selection axes incl. `QuotaExhausted`/`QuotaShortfall`/`CaptchaRequired`, one-way `BeforeCreate → GenerationCreated` attempt loop, bounded event+post-turn-billing FIFO (Codex/Gemini pairing); `ProviderMode::Suno` + `serves_suno()`, metrics None-arm, server bounded-404 placeholder; 69 new `suno::` tests |
| Runtime gateway + uploads + durable billing | 9665b1fd | `crates/forward/src/suno/{gateway,session,artifacts,upload}.rs` + `billing.rs` Suno writer/reader commands: per-profile JWT-mint/`set-cookie` re-seal single-flight (winner re-seals before releasing, loser re-reads; rotation merge is position-preserving so a noop merge re-seals nothing), reserve (song 5 published / 50 conservative for extend-lyrics-stems) → hCaptcha pre-check (never solved) → attribution baseline → create → detached bounded poll (feed/clip/lyrics) → immediate artifact download (tmp+fsync+0600+rename) → settle attributed delta else reserve with `native_schedule_derived` + unattributed counter → FIFO event+post-turn-quota pairing (Codex/Gemini discipline); window identity evidence-derived from strict `YYYY-MM` period (absent → no observation, turn still persists); admission matrix covers all four §4 operations with the admitted set named on 400; attachments fail closed (`suno_attachment_upstream_unknown`) with a durable ≤96 MiB intake; corroborated Clerk streak+elapsed removal from routable; mock-upstream tests: happy-path exact settle, CAPTCHA rotation, ambiguous reserve settle, zero-movement refund, zero-quota hard wall 429, pre-create-only rotation, roster last-good, projection privacy, upload intake; `forward/CLAUDE.md` Suno section |
| Server wiring + admin projection + observability | (this commit) | `crates/server/src/{config,main,poller,http}.rs`: `CLAUDE_API_SUNO_*` strict default-off config (BASE_URL rejected as unknown key, even dormant), `CLAUDE_API_PROVIDER=suno` composition requiring the PG billing authority (degraded zero-capacity gateway on a broken initial roster), 15 s discovery + quota sweep loop, shutdown drain contract; routes `/suno-subs` + `POST /v1/audio/generations` + `POST /v1/audio/uploads` + `GET /v1/audio/generations/{id}[/artifact/{name}]` with per-route body limits and a bounded-404 fallback; `/ready` tracks gateway readiness; 23 fixed-cardinality `claude_api_suno_*` series (zero-label pins by test); `suno-provider` alert group (7 alerts, all gated on `claude_api_suno_enabled == 1`) + same-named MONITORING.md sections + monitoring-config.test.sh pins; charge-mismatch collector provider list gains `suno`; `docs/DEPENDENCIES.md` roster row (engine re-seals on rotation); `crates/server/CLAUDE.md` switch/routes/shutdown contract; 7 new server tests |

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

Admin control room consumer (`apps/admin`, separate change) or the safe live-runner
(`tools/suno_calibration/`, `docs/ops/SUNO_CALIBRATION.md`). The engine plane is fully wired:
runtime (forward), server composition, admin projection, metrics and alerts are delivered and
dormant; the live matrix stays blocked on a human-owned subscription.

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
