# crates/server — CLAUDE.md

**Role:** COMPOSITION (bin `claude-api`). Reads env, raises the pool from the registry, starts the
background loops and the HTTP router. Here — and only here — everything is wired together.

**Owner branch:** `comp/server`.

**Boundaries (hard):**
- Depends on `forward`, `pool`, `registry`, `tokio`, `axum`, `clap`.
- **The ONLY place where the environment is read** — `src/config.rs` (`Settings::from_env`).
  Nothing below this layer touches env.
- Contains no forwarding business logic (it lives in `forward`) and no selection logic (it lives in `pool`).
  Here is the wiring: env → `ProxyConfig`/`Pool`/`Clients` → `AppState` → router + loops.

**What's inside:**
- `config.rs` — `Settings` (db_path/bind/fleet/Redis affinity + `ProxyConfig`) from env.
- `http.rs` — the router: `/health`, `/pool`, `/balance`, `/capacity` (control endpoints) + startup-fixed
  Claude/OpenAI/Gemini router. The production provider is selected by the systemd unit, not the request; the Caddy marker remains
  only in the one-shot `Combined` migration bridge and is never accepted from a client. +
  data routes for admin.apitoken.sale (`/overview`, `/capacity`, `/subs` etc.; UI — standalone
  Next.js `apps/admin`, architecture — root `docs/product/PANEL.md`) +
  `/admin/*` (control plane,
  see `admin.rs`) + fallback to `forward::forward`. Key issuance returns the non-secret `key_id`,
  and `/admin/key-id/{key_id}/status` allows revoking a key without passing the full secret again.
  A single provider-process admission layer applies only to customer provider routes for
  `Combined|Anthropic|OpenAi|Gemini`: it consumes the reserved logical-request-ID header and optional
  public `x-apitoken-client`, attaches typed request context, and runs before
  auth/body/reserve/dispatch. Malformed logical identity is rejected because public Caddy has already
  removed internet values: it represents a broken trusted internal capability, not customer
  credential input. Client attribution is not authority: invalid, duplicate, unsupported, or absent
  evidence fails open to typed unknown and never changes the existing HTTP/auth/body response.
  Health/admin/internal router preflight and backend-only KIMI/Tripo3D/Suno stay outside this MVP;
  OPTIONS on a customer provider route is admitted like every other method; the capability is
  removed before existing method/fallback semantics, so it cannot escape to an external upstream.
  `/metrics` exports the registry incident tripwire
  `claude_api_execution_group_double_winner_total`; the metric must stay at zero, and
  transactional winner correctness does not depend on the process or Prometheus. The fixed-cardinality
  `claude_api_execution_not_started_total{plane}` in the same place counts only an exact single `not_started` on a
  non-2xx response actually returned by the concrete Anthropic/OpenAI/Gemini plane; the Combined bridge
  attributes the old Caddy OpenAI marker, otherwise Anthropic.
  Identical on all fixed planes, `POST /internal/router/policy/preflight` is a loopback-only
  producer contract of router phase 6.4a: it accepts up to 32 catalog candidates, authorizes the customer/admin
  credential and returns only an ordered allow-list without any account/policy/pricing identity.
  The bodyless `POST /internal/router/auth/preflight` preceding it is a producer-first read-only
  contract of early universal admission: it verifies the customer/forwarding-admin credential before the router reads
  a large request body, does not read the prompt, does not reserve money and returns only a
  closed `{schema_version:1,authenticated:true}` or 401/503.
  A separate producer-first `POST /internal/router/catalog/pricing` accepts up to 256 provider-native
  catalog candidates and returns only an opaque candidate ID plus personalized integer
  nanoUSD-per-million rate cards. The customer credential is resolved via `AsyncBilling`; a legacy
  account uses the live `mult_bp`, a strict account uses the same coherent bundle/resolver and payable
  multiplier as admission. Tariff baskets are taken only from `metering`; the endpoint is read-only,
  does not reserve money and does not return credential/account/policy/rule identity. It is mounted on
  every fixed plane before the consumer in `crates/router` is wired in.
- `admin.rs` — **Control API** (`/admin/account`, `/admin/key`, `/admin/*/credit|status`): the contract
  by which the FUTURE COMMERCE (a separate service) manages the engine. The gate is `forward::control_authed`
  (the control key, SEPARATE from forwarding-admin). All writes go through the single-writer actor `AsyncBilling`
  (the same discipline as reserve/settle). The engine remains the authority on the LIVE balance; commerce only
  creates accounts/keys and credits (idempotently by `ref`). The full contract is `docs/engine/CONTROL_API.md`.
  Account pricing is updated by `/admin/account/{id}/pricing` (the default discount) and
  `GET|POST /admin/account/{id}/discounts` (per-provider overrides — the whole B2B pricing
  surface, `docs/commerce/PRICING_MODEL.md`); cursor ledger reads use `after_id` for
  the commercial pricing worker. Charge rows additively expose non-negative
  `uncollected_nano` (missing from an older producer means zero): `amount_nano` stays full billed
  actual, and the consumer derives the collected debit by subtraction so pool loss cannot become
  customer funding or partner commission. Account reads include the coherent paid/bonus/other/unattributed
  funding summary. Ledger rows add stored immutable attribution and normalized funding allocations;
  old rows remain null/empty and are never reclassified at the HTTP boundary.
  Hot tariff overrides are exposed under `/admin/pricing/tariffs*`: list, a read-only compiled
  catalog dump built from `metering` (authority-free) with time-relative `has_future_epoch` and
  whole-schedule `seed_safe`, next-version publish (server computes `head + 1`, one sequence-race
  retry) and idempotent version-2 seeding that never overwrites a family whose head is past version
  2. Any selected multi-epoch family makes seed fail atomically with 400 before authority access;
  it must use explicit effective-dated override rows. The metering→registry payload converters live in
  `tariff_admin.rs`; writes go through the billing writer actor, reads through the reader pool,
  and the authority is PostgreSQL-only (SQLite answers 503). Contract — the hot tariff overrides
  section of `docs/engine/CONTROL_API.md`. The runtime consumption of the table lives in
  `crates/forward` (`pricing/tariff_book.rs`): server installs the kill switch
  `CLAUDE_API_TARIFF_OVERRIDES` (on/off, default on; off = the book always answers empty and every
  price is the compiled constant) and spawns the 5-second refresher loop only on the PostgreSQL
  authority.
- `poller.rs` — EVENT-DRIVEN loops: `reload_loop` (re-reads the registry; wakes the poller via `Notify` when
  the fleet changes) + `poll_loop` (free count-tokens probe of matured subscriptions concurrently, then sleep
  EXACTLY until the nearest due time or until a `poke`). There is no fixed tick: reset is computed
  locally. Under live traffic, passive headers usually keep `polled_ts` fresh, but the completion of an
  authoritative Claude turn after the exact spend is queued to FIFO forcibly marks the subscription due
  and wakes the poller for post-turn quota pairing. `LIVENESS_INTERVAL` sets the rare background check of an
  idle subscription, and a per-sub 15-second debounce bounds forced probes. If a Claude sub
  lacks a plan, the same backend probe first reads the official OAuth profile. For an inference-only
  token with HTTP 403 only a fail-closed fallback is allowed: all non-empty plans of the active fleet must
  unanimously match one `pro|max5|max20`. The result is durably persisted and immediately updates the
  in-memory roster; a mixed/unknown fleet stays without a plan, the UI does not participate in detection.
  `persist_loop` — write-through persistence of pool state on a cooling event (`pool.on_change` → `Notify`)
  + a rare safety flush; at startup `serve` restores state via `pool.import_state`.
  If import quarantined implausible legacy calibration, `serve` immediately wakes the persist loop so the
  repaired prior-fallback does not remain merely in-memory until the safety flush.
  `poll_loop` also maintains **durable auth-health**: the probe feeds `pool.record_probe` (the dead-detection state machine),
  a changed verdict is persisted owner-fenced (`save_sub_health`); suspect/dead are probed INDEPENDENTLY
  of cooling (`SUSPECT_INTERVAL`/`DEAD_RESURRECT_INTERVAL`) to gather corroboration/resurrection. At
  startup `serve` seeds the verdict via `pool.import_health` (dead subs are immediately out of rotation and survive a restart).
  `/metrics` exports aggregate `claude_api_anthropic_auth_{suspect,dead}_subscriptions` gauges:
  request-path 401/403 remain a separate diagnostic counter and never constitute credential death.
  A separate Gemini health loop every 15 seconds discovers new roster profiles and, on the configured
  cadence, checks health/quota. After a durable-settled admin-only exact-target turn it receives a
  coalesced `Notify` and immediately performs a free probe; ordinary customer turns do not send this wake.
  Gemini Batch stays independently default-off in config. Stage 6 activates both runtime and public
  facade only through `systemd/claude-api-gemini@.service`; the dedicated encrypted data keyring is
  read from root-owned `server.env`, never embedded in the unit, and the legacy singleton rollback
  unit does not enable Batch. Discovery advertises `batchGenerateContent` only when that public
  facade was actually composed.
  A separate KIMI maintenance loop every 15 seconds discovers the Auth Bot's atomic publication, and
  on `CLAUDE_API_KIMI_QUOTA_POLL_SECS` runs a free `/usages` sweep (the first anchor — immediately
  after preflight). The gateway itself validates roster generation, the idle/epoch boundary of a quota snapshot,
  the turn-FIFO→durable spend→observation/CAS order and last-good publication; server owns only the
  cadence. A failure of one profile does not stop the sweep of the others.
  The GLM maintenance loop is a mirror of KIMI (`docs/engine/GLM_PROVIDER.md` §5.4): the same independent
  15-second roster discovery and free quota sweep on `CLAUDE_API_GLM_QUOTA_POLL_SECS`;
  the gateway owns the idle/epoch boundary, the turn-before-quota ordering and the durable observation/CAS,
  server owns only the cadence.
- `main.rs` — clap CLI: `serve`, `sub add/add-file/list/rm/status/proxy/fleet/set-plan/detect-plan/health`,
  the private `openai-image-canary`, and the one-shot `openai-image-public-smoke`. The private canary
  can freeze an explicitly named opaque Codex profile or the first currently admitted pool profile.
  The temporary read-only settlement diagnostic used by two fenced 2026-08 deliveries is retired;
  its terminal evidence remains in the immutable deployment statuses. None of these commands is part
  of `AppState` or HTTP routing.

**Invariants:**
- At startup the PostgreSQL authority only read-only verifies the applied schema; DDL is executed
  by the separate `db migrate-engine` before a blue-green slot is started.
- Introduce a new env variable ONLY here and pass it further down through config structures. The
  dormant `CLAUDE_API_*BODY*_MIB` envelope is parsed strictly through `api-limits`: malformed, zero,
  inconsistent, or above-current values stop startup. It cannot widen Anthropic 32 MiB, Codex 8 MiB,
  Gemini text 32 MiB/media 20 MiB, translated 32 MiB, or Gemini-native 64 MiB caps before later stages.
- `openai-image-canary` introduces no image key/origin/env. Dry-run validates the strict prompt,
  optional one-to-five PNG references, optional opaque profile, private target paths, numeric
  budget and the `low|medium|high` quality knob (default `low`), then prints a sanitized plan without
  reading env/network or creating artifacts. Generation is ready only at an explicit budget covering
  the exact official output-token ceiling of the requested quality — `22_330_000` nanoUSD for `low`,
  `180_460_000` for `medium`, `714_130_000` for `high`, from the exhaustive 16/48/96-cell maxima of
  659/5,930/23,719 image tokens over every request-valid resolution — plus the conservative
  512-text-token prompt allowance. `--execute` additionally requires an exact compile-time
  implementation SHA, reuses the existing Codex OAuth roster/base URL/identity, freezes one admitted
  profile, runs its free `/wham/usage` auth/quota preflight, and performs one exact-home attempt with
  the requested quality. A successful checkpoint requires the returned quality to equal the requested
  one, opaque metadata, a bounded native auto-size PNG and terminal usage; a normalized tier instead
  lands in the sanitized mismatch journal with the returned controls, dimensions, numeric usage,
  optional request id, and the image SHA-256 — the probe verdict — and never persists or publishes
  the rejected image. Edit validation accepts one to five PNG references; paid execution authorizes
  the whole published 8,000,000-TPM envelope per reference plus the generation ceiling
  (`64_022_330_000` nanoUSD for one reference at `low`) because OpenAI publishes no GPT Image 2
  high-fidelity input formula — an absolute authorization envelope, not an expected price.
  Output/checkpoint publication uses exclusive mode-`0600` files. The private command remains outside
  `AppState`, while
  `http.rs` now mounts producer-first `/v1/images/generations|edits` only on the OpenAI plane (and the
  header-gated Combined bridge), with 256 KiB JSON and 17 MiB multipart route limits. The model is
  published (pricing catalogs, site, unified-router native lane); `/v1/models` deliberately does not
  list it. Image auth runs
  inside the handler before JSON or multipart extraction, so unauthenticated bodies are never buffered.
  Generation and edit evidence are watchdog-GREEN. `openai-image-public-smoke` adds no key, origin,
  fallback or env: dry-run reads no env/network, while `--preflight-only` and `--execute` require an exact
  compile-time SHA and a new absolute output path under an existing actual mode-private directory. Both
  modes read only `CLAUDE_API_DATABASE_URL` through the narrow `config.rs` helper rather than assembling
  unrelated server/provider settings, borrow exactly one active unexpired key belonging to the unique
  engine account handle `crm-parsing` (not an account whose opaque ID equals that handle) whose active
  assignment and linked policy are service/meter-only, without serializing it, and require image aliases
  absent from authenticated discovery.
  Every database/schema/credential/runtime/discovery stage is persisted before it starts with both dispatch flags
  false. `--preflight-only` stops at `preflight_success`; `--execute` repeats that fresh free preflight,
  sends one public generation and then one one-reference edit with no post-dispatch retry, and correlates
  each lowercase UUIDv4 response identity to an exact release-v2 snapshot, reservation, usage row, outbox
  completion and settlement. The orchestration is synchronous: each HTTP future enters the command's Tokio
  runtime only for that network operation, then exits it before the synchronous PostgreSQL client is queried or
  dropped. Never call `PgStore` from inside this command's Tokio runtime; the synchronous `postgres` client owns
  a separate runtime and nested `block_on` panics. Each settlement uses a 150-second wall-clock evidence deadline
  with 500 ms polling; the shared PostgreSQL session already bounds each statement to 15 seconds and each lock
  wait to 5 seconds. A timeout is terminal and never replays the paid request. Success requires exact official
  token/nanoUSD legs, `charge_nano=0`, unchanged account/key money aggregates, bounded
  byte-different mode-`0600` PNGs and mode-`0600` evidence in a mode-`0700` replay-fenced directory.
  Full contract — `docs/ops/GPT_IMAGE_2_CANARY.md`.
- ClaudeStore emergency transport: `CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED` strict default-off
  (`0|1|false|true`), the secret `CLAUDE_API_CLAUDESTORE_API_KEY` is required only when enabled and undergoes
  shape-validation/redacted Debug. Enable is allowed only for `Combined|Anthropic`; its production base
  URL `https://api.llmsrelay.com` is compile-fixed in `forward`, with no env override. The secret lives only in the root-owned
  `server.env`, and the runtime contract is `docs/engine/CLAUDESTORE_FALLBACK.md`.
- The GPT transport uses the independent `CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED` and
  `CLAUDE_API_CLAUDESTORE_CODEX_API_KEY`. Enable is allowed only for `Combined|OpenAi`, additionally
  requires `CLAUDE_API_CODEX_ENABLED=1`, remains compile-fixed to
  `https://api3.claudestore.store`, and never reuses the Basic/Claude key. The fixed OpenAI
  systemd unit inherits this switch; the Anthropic/Gemini units must pin `0` at argv level.
  Having a valid config neither closes the authenticated live gate nor permits a production enable.
- The backend-only KIMI switch is read here as a strict default-off set
  `CLAUDE_API_KIMI_{ENABLED,ROSTER_DIR,CREDENTIAL_KEYS,BASE_URL,AUTH_SCHEME,QUOTA_POLL_SECS}` and is
  passed whole to `forward::kimi::config::build`. A disabled plane does not validate dormant
  values; an enabled plane fail-closed requires an absolute roster, an encrypted keyring, HTTPS,
  a known auth scheme and a positive poll interval. Exact KIMI aliases are served only by
  authenticated profiles; an initially degraded gateway and a failed reload keep a separate
  fail-closed KIMI path without affecting Claude readiness and without dropping the alias into the Claude pool.
  `ProviderMode::Kimi` (`CLAUDE_API_PROVIDER=kimi`) is a dedicated delivery plane: production
  units `systemd/claude-api-kimi@.service` (active/passive slots 8804/8805 behind the stable loopback
  origin 127.0.0.1:8803) and the legacy/anchor singleton `systemd/claude-api-kimi.service` (8804).
  Both units pin `CLAUDE_API_KIMI_ENABLED=1` at argv level: the plane state lives only in
  reviewed units; turning it off is a reverse reviewed change.
  The Anthropic plane keeps KIMI disabled so that exactly one process runs the maintenance writer.
  In kimi mode the router mounts only the common routes, `/kimi-subs` and `/v1/messages`, which
  dispatches exact KIMI aliases through the same `KimiGateway::handle` as the Anthropic path, while any
  other model gets a bounded fail-closed 404 — the Claude pool is not raised on this plane.
  `/ready` here is accepting && authority_ready && (gateway present ? its readiness (live>=1 &&
  persistence_ok → otherwise provider_unavailable) : ready-to-serve-disabled-envelope).
- The backend-only GLM switch is read here as a strict default-off set
  `CLAUDE_API_GLM_{ENABLED,ROSTER_DIR,CREDENTIAL_KEYS,AUTH_SCHEME,QUOTA_POLL_SECS}` and is
  passed whole to `forward::glm::config::build`. The absence of a fleet base-URL override is intentional: the console
  origin is per-profile inside the sealed credential (int/CN keys are incompatible,
  `docs/engine/GLM_PROVIDER.md` §2), so `CLAUDE_API_GLM_BASE_URL` is rejected fail-closed as an
  unknown key rather than ignored as dormant junk. A disabled plane does not validate dormant
  values; an enabled plane fail-closed requires an absolute roster, an encrypted keyring, the
  `bearer` scheme (the only proven one) and a positive poll interval. As with KIMI, an initially degraded
  gateway and a failed reload keep the fail-closed GLM path without affecting Claude readiness and
  without dropping an exact GLM alias into the Claude pool. The GLM plane's client fingerprint has NO
  GLM-specific env: the persona identity is filled here from the SAME shared fleet env as the Claude persona
  (`CLAUDE_API_IDENTITY`, `CLAUDE_API_UA` (+ pool via `|`), `CLAUDE_API_BETA`,
  `CLAUDE_API_ANTHROPIC_VERSION`, `CLAUDE_API_X_APP`, `CLAUDE_API_SL_*`, `CLAUDE_API_CC_VERSION`,
  `CLAUDE_API_CC_ENTRYPOINT`, `CLAUDE_API_INJECT_BILLING`), which is auto-refreshed by
  `tools/refresh-fingerprint.sh`; the per-field fallback is the reviewed capture of 2.1.195 in
  `GlmIdentityHeaders::default`. GLM intentionally does not read `CLAUDE_API_UA_SPREAD`: the patch-level UA
  spread was removed from the Claude persona as a source of within-request anomalies (`persona_ua` in
  `forward::upstream`); there is nothing to mirror.
- Pricing has no env surface: an account carries one discount, overridden per provider where the
  customer's terms differ, and both live in the engine authority. The retired
  `CLAUDE_API_PRICING_BRIDGE_*` and `CLAUDE_API_PRICING_SHADOW_*` variables are gone; the fleet env
  file may still carry them and the server ignores them.
- `POST /admin/key` and reactivation via `/admin/key-id/{key_id}/status` accept a nested
  `activation_policy_ack {effective_policy_version, policy_digest}`. For a strict binding an exact ACK
  is mandatory; a missing/stale/wrong identity yields 409, a malformed identity — 400. Disable does not
  require an ACK. The key secret is still issued once and only after the durable ACK check.
- Redis is only configured here; `AffinityStore` lives in `forward`, and pool stays network-free.
- Router policy preflight answers `unrestricted` for every authenticated key: every model of every
  enabled provider is available to everyone, and pricing is a discount, never an allow-list. It
  still authenticates through `AsyncBilling` and returns no credential/account identity.
- Router auth preflight uses the same `authed`/`resolve_client_key` as live admission: an
  inactive/unknown credential gets 401, a billing authority failure — 503, and success does not reveal
  key/account identity and does not reserve/settle. The endpoint is identical on all fixed planes and
  loopback-only; `crates/router` calls it before materializing each universal request body.
- The control endpoints (`/health`, `/pool`, `/capacity`, `/fleet-history`, `/settlement-health`,
  `/codex-subs`, `/gemini-subs`, `/kimi-subs`, `/glm-subs`, `/tripo3d-subs`, `/suno-subs`, `/admin/*`) live here; everything else → forwarding. `/capacity`,
  `/codex-subs` and `/gemini-subs` serialize a safe paid-plan identity for the protected
  `admin.apitoken.sale/sales/calculator`; this classifier is not a credential. `/codex-subs`
  is gated by `control_authed` and returns only an opaque home id plus a bounded email hint (the first four
  characters of the local part without the domain), never the full ChatGPT email/account id/OAuth/proxy. Windows explicitly
  publish the provider measurement resolution, and `plan_cohorts` merges only exact paid plan +
  duration into a shared native-credit capacity per home/fleet; per-home evidence and workload-dependent
  API USD are not replaced by this aggregate. Provider subscription objects also expose nullable
  `acquired_at`/`subscription_expires_at`/`subscription_days_left`: Claude comes from registry `added_ts`
  joined by full identity before masking, Codex from sealed `issued_at` +30 days, and Gemini from sealed
  `issued_at` (+18 UTC calendar months for `google_ai_pro`, +30 days for other canonical plans).
  `/capacity` publishes Claude 5h/7d and horizon money as decimal nanoUSD strings, per-sub remaining and
  the authoritative conversion catalogue from `metering`:
  Standard for the seven canonical models, Fast only for the actually supported Opus 5/4.8.
  Claude full-window capacity is pooled only within an exact plan+duration by the formula
  `10^8*Σspend/Σfraction`; a different routable plan without evidence, a snapshot older than 900s or pending/
  degraded calibration delivery fails closed for fleet remaining. Historical capacity is not
  erased in that case. Current per-sub/fleet remaining may use a fresher ephemeral
  `pool::QuotaSnapshot`: the exact fixed-point utilization from a response/count-tokens probe remains
  useful even if the provider did not send a reset. Such a snapshot lives only in runtime, goes stale after
  900s and never becomes estimator/history evidence; horizon availability stays `null` without a
  real reset. Until the exact future provider deadline, the last snapshot of a routable-idle or
  quota-cooling home remains a separate display state: fraction/reset and
  `last_known_remaining_nano` are visible to the operator, but `snapshot_fresh=false`, canonical remaining and
  saleable fleet/horizon money stay `null`. A new snapshot replaces it, and an elapsed reset
  removes it so the old value does not migrate into the new window. Pending/degraded delivery does not publish
  this display state. `calibration_delivery` exposes only bounded queue
  counts/integrity, without identity.
  `/capacity` also returns newest-first `calibration_recent_turns` with at most 512 immutable
  Anthropic events: opaque request ID, masked email and the full token/cost vector without prompt/credential.
  This is backend evidence for the operator runner; the aggregate `calibration_evidence` remains statistics.
  `/gemini-subs` symmetrically publishes only the new plan-scoped exact authority: independent 5h/
  weekly rows and fleet totals, `calibration_authority_available`, bounded Gemini FIFO health,
  exact model/token/API-cost aggregates and newest-first at most 512 immutable Google turn events.
  Pending/dropped/degraded delivery makes Gemini fleet remaining unavailable rather than a saleable stale
  number; legacy pre-plan Gemini calibration is not mixed into this authority.
  `/overview` and the new metrics.db snapshots take capacity-facing fields from the same exact report;
  `pool::Cap` prior/EMA remains routing-only. Overview adds canonical decimal `*_nano`, while its
  old float USD fields remain only for display compatibility.
  `/fleet-history` reads metrics.db history
  (snapshots/sub_snapshots for 24h/7d/30d/90d, bucketed to ≤ ~500 points, an optional
  per-sub series by an email mask) and is gated by `control_authed`, like `/overview` with money
  aggregates. `/settlement-health` is money diagnostics of the settlement pipeline: counts of
  settlement_outbox by state (pending/done/failed; 'processing' exists in the schema but is never written),
  failed total/24h, backlog of unsettled older than 300s, ≤10 latest failed with last_error
  truncated to 200 characters (settle errors — invariant/SQLSTATE details, no secrets), and the lag of the
  pricing consumer (max(ledger.id) vs ledger_consumer_checkpoints, the age of the oldest
  unconfirmed row); read via registry (`PgStore::settlement_health` / SQLite twin
  in registry::settlement_health) — server never touches PG directly. `/spend-stats` accepts
  optional `from`/`to` (epoch seconds, together): the response is extended with a `custom` block for
  the half-open range [from, to) up to 92 days wide (garbage/from ≥ to/future/wider than the limit — 400;
  `to` is clamped to now+1); `custom` is computed on every request, bypassing the TTL cache, which holds
  only the standard windows d1/d7/d30. Range aggregations go through registry `spend_by_*_range`. `/gemini-subs` exists
  only in the fixed Gemini runtime, is gated by
  `readonly_authed` and serializes opaque ids, a bounded email hint, quota/cooling, per-model
  generation health and low-cardinality failure classes plus separate gaxios and Undici transport attestations and
  the Antigravity version — without Google subject/full email/domain, project/proxy/OAuth. The response also
  publishes exact nanoUSD fleet totals, the paid-tier conversion catalogue from `metering::gemini` and the
  canonical-model → private quota-bucket mapping; a missing provider amount stays `null`.
  `/kimi-subs` exists in Combined and Anthropic modes (the embedded gateway is dev/test-only) and on
  the dedicated Kimi plane (production, origin 8803), gated by `control_authed`, not the panel key. The response is either a
  disabled envelope `{"now", "enabled": false, "profiles": []}` or a read-only operational
  projection: bounded FIFO delivery, fleet counts and per-profile cooling/inflight/quota-window
  state plus per-window durable calibration (capacity/remaining as decimal nano strings).
  For the live runner the envelope carries `calibration_authority_available`,
  `calibration_recent_turn_limit`, immutable `calibration_recent_turns` (engine request id +
  opaque profile id + bounded plan + exact usage/nano legs) and `conversion_models` with the official
  rate card. Dispatch supports the admin-only pair `x-apitoken-calibration-{profile,request-id}`:
  only together, only under the admin key, never upstream; a half-pair/non-admin/garbage — 400, and the
  pinned turn goes exactly to the named profile without rebind.
  Only opaque roster ids and reviewed bounded plan labels are serialized (`"unreviewed"` for
  any unreviewed provider plan string); subject, email, phone, token, proxy, credential path,
  customer/request id and raw provider errors are never serialized; unknown is `null`,
  not 0. The join durable subject→opaque id is performed inside the HTTP layer via the gateway; the subject itself
  never enters the response; rows of a foreign subject remain durable for audit but are not published.
  `/glm-subs` — the same contract for the backend-only GLM plane (Combined and Anthropic modes,
  `control_authed`, a disabled envelope without the plane). Differences from the kimi form reflect the GLM axes:
  GLM has no timed auth quarantine — instead of `cooling.auth_until` the profile carries durable flags
  `account_dead`/`account_suspect`; raw quota counters are serialized as `null` while their unit
  semantics are unproven; calibration is dual-ledger (decimal nanoUSD strings + exact native
  microcredits); the envelope adds `window_totals` — a fleet aggregation of the canonical 5h/7d windows
  (`window_minutes` 300/10080 as a projection of the exact `duration_secs`, capacity/remaining as decimal
  nanoUSD strings), where the aggregate is `null` while at least one roster profile has not named a value for
  the window — a partial sum is never published. The subject here is a keyed digest of the key; it,
  the key itself, proxy and base_url are not serialized, and the join subject→opaque id is likewise performed inside the
  HTTP layer via the gateway.
- **Three key classes (secret separation):** `CLAUDE_API_KEYS` (forwarding-admin: unmetered /v1
  + everything), `CLAUDE_API_CONTROL_KEY` (control plane `/admin/*`: accounts/money, for commerce),
  `CLAUDE_API_PANEL_KEY` (read-only dashboards `/capacity`,`/metrics`). Gates: `authed` (admin) ⊂
  `control_authed` (admin|control) ⊂ `readonly_authed` (admin|control|panel).
- `/health` without authorization (bare liveness); `/pool` — `authed`; `/capacity`,`/metrics` —
  `readonly_authed`; `/fleet-history`, `/settlement-health` and `/admin/*` — `control_authed`.
- Fixed OpenAI `/ready` additionally checks the provider snapshot: any transport requires at least
  one live+authenticated home. One working subscription remains real capacity and does not turn into
  a 503 because of pool size; both blue-green generations read one sealed roster, so parity of the
  authenticated-home set at cutover is guaranteed by construction, without a minimum soak interval.
  Fixed KIMI `/ready` with an assembled gateway requires live>=1 && persistence_ok
  (otherwise `provider_unavailable`); with no gateway (argv-pinned default-off) the slot stays
  ready and serves the stable disabled envelope.
- `/metrics` publishes privacy-safe affinity counters, including soft cache-root hits/writes, and
  fleet-only Anthropic exact-capacity/coverage/delivery gauges, as well as three
  execution-not-started series.
  Raw client IDs, prompt content, account IDs, model IDs, credential/group/request identity and
  subscription IDs never reach Redis/metrics.
- **loopback trust is explicit opt-in only** `CLAUDE_API_TRUST_LOOPBACK=1` + a real loopback bind
  (otherwise behind a reverse proxy an anonymous caller would get admin access).
- OpenAI shutdown first waits for detached Codex stream/history/settlement tasks (the native provider
  holds no child processes — the abort signal tears the upstream read on the deadline); only after that
  may the billing FIFO flush terminate the process.
- Gemini shutdown first closes admission and waits for the detached SSE drain; on the deadline the abort signal
  interrupts the upstream read, the task settles the last usage snapshot and crosses the semaphore barrier.
  The billing FIFO flush is allowed only after that. Gemini health/preflight/network live in `forward`,
  while the env/upstream pin and the startup-fixed service composition live only here. The production unit must
  argv-level pin the sealed credential layout + Antigravity version + Cloud Code host + Node
  binary/version/SHA after the shared EnvironmentFile. `systemd-flat` is a closed alternate retained
  only for an explicitly operator-launched exact-SHA calibration process; no installed production
  unit selects it. Its roster and flattened envelopes arrive through systemd credentials and retain
  exact-path/private-mode checks in `forward`.
- KIMI shutdown closes admission and steady maintenance, cancels a pending quota GET, waits for the detached
  stream drain and the turn FIFO; on the deadline it aborts the stream read, saves the conservative settlement,
  then under the same deadline performs a final turn-before-`/usages` pass. Neither the old poll nor the
  final provider read may outlive the shared billing flush.
- GLM shutdown mirrors KIMI: closes admission and steady maintenance, waits for the detached stream drain
  and the turn FIFO, on the deadline aborts the stream read, then under the same deadline performs a final
  turn-before-quota pass against the GLM quota endpoint. Neither the old poll nor the final provider read
  may outlive the shared billing flush.
- Tripo3D shutdown follows the same shape adapted to the task lifecycle: closes admission and steady
  maintenance, waits for the detached task drains (poll → artifact download → exact settlement → paired
  FIFO), and on the deadline stops the drains mid-poll — the reservation stays with its lease and the
  reconciler, never a settlement from ignorance — then runs the final turn-before-balance pass under the
  same deadline, inside the shared billing flush.
- The backend-only Tripo3D switch is read here as a strict default-off set
  `CLAUDE_API_TRIPO3D_{ENABLED,ROSTER_DIR,CREDENTIAL_KEYS,CREDENTIAL_ACTIVE_KID,BALANCE_POLL_SECS,ARTIFACT_DIR}`
  and is passed whole to `forward::tripo3d::config::build`; it composes only in
  `ProviderMode::Tripo3d` (`CLAUDE_API_PROVIDER=tripo3d`), which mounts `/tripo3d-subs` and the
  bounded task surface (`POST /v1/3d/generations`, `POST /v1/3d/uploads/{image,model}`,
  `GET /v1/3d/tasks/{id}[ /artifact/{name}]`) and answers every other path with a bounded 404 —
  there is no Claude pool to fall into. The absence of a fleet base-URL override is intentional:
  the platform origin is per-profile inside the sealed credential (global and CN keys are not
  interchangeable, `docs/engine/TRIPO3D_PROVIDER.md` §2), so `CLAUDE_API_TRIPO3D_BASE_URL` is
  rejected fail-closed as an unknown key rather than ignored as dormant junk. A disabled plane
  does not validate dormant values; an enabled plane fail-closed requires an absolute roster and
  artifact directory, an encrypted keyring (the active kid, when set, must exist in it) and a
  positive poll interval. No production units or Caddy routes exist for this plane yet — the
  activation boundary is dormant by design (manifest §8).
- Suno shutdown follows the same task-lifecycle shape as Tripo3D: closes admission and steady
  maintenance, waits for the detached generation drains (poll → artifact download → settlement
  from the attributed credit delta, else the documented reserve → paired FIFO), and on the
  deadline stops the drains mid-poll — the reservation stays with its lease and the reconciler,
  never a settlement from ignorance — then runs the final turn-before-quota pass under the same
  deadline, inside the shared billing flush.
- The backend-only Suno switch is read here as a strict default-off set
  `CLAUDE_API_SUNO_{ENABLED,ROSTER_DIR,CREDENTIAL_KEYS,CREDENTIAL_ACTIVE_KID,QUOTA_POLL_SECS,ARTIFACT_DIR}`
  and is passed whole to `forward::suno::config::build`; it composes only in
  `ProviderMode::Suno` (`CLAUDE_API_PROVIDER=suno`), which mounts `/suno-subs` and the bounded
  audio surface (`POST /v1/audio/generations`, `POST /v1/audio/uploads`,
  `GET /v1/audio/generations/{id}[ /artifact/{name}]`) and answers every other path with a
  bounded 404 — there is no Claude pool to fall into. The absence of a fleet base-URL override
  is intentional: the provider has one platform with fixed official hosts
  (`docs/engine/SUNO_PROVIDER.md` §2), so `CLAUDE_API_SUNO_BASE_URL` is rejected fail-closed as
  an unknown key rather than ignored as dormant junk. A disabled plane does not validate dormant
  values; an enabled plane fail-closed requires an absolute roster and artifact directory, an
  encrypted keyring (the active kid, when set, must exist in it) and a positive poll interval.
  No production units or Caddy routes exist for this plane yet — the activation boundary is
  dormant by design (manifest §8).
- Claude shutdown, after the stream drain, calls the shared billing FIFO barrier: a pending calibration head
  is retried until the outbox reconcile; the process does not declare the flush successful while exact evidence remains
  unapplied.

**Verification:** `cargo build -p claude-api`; `cargo run -p claude-api -- serve`.
