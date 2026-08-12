# Suno — provider capability manifest

Integration status: **research complete; default-off backend preview ahead; nothing is published**
(no production defaults, no public catalog, no router presets, no storefront, no public docs).
Source review date — **2026-08-12**.

This document was created per `docs/engine/PROVIDER_ONBOARDING.md` §3.3 and is the capability
manifest of the Suno plane. Every claim is labeled per the evidence hierarchy from §3.1:
`official`, `live`, `oss-hypothesis`, `decision`, `unknown`, `not-applicable`. The mechanical edit
map is `docs/engine/PROVIDER_WIRING_CHECKLIST.md`; the closest reference is
`docs/engine/GLM_PROVIDER.md`.

## 0. Scope and intentional limitations

The Suno plane is built **backend-only**, per the product owner's frame: engine runtime,
metering, calibration, Auth Bot, admin control room — and **no publication** to the public
catalog, router presets, commerce/OpenKeys pricing, the site, or client docs. Two independent
facts force this beyond the owner's instruction:

- `official` Suno has **no public official API**. `platform.suno.com` exists (partner-gated,
  Google sign-in, no public docs/pricing/keys; announced 2026-07-01 as a curated partner program,
  intake via Typeform). `api.suno.com/health` answers but every probed API path 404s. The only
  technically working access is **session-cookie pooling against internal web endpoints**
  (§4), and —
- `official` the Terms of Service (`suno.com/terms`, last revision 2026-03-26; new ToS effective
  2026-09-03) unambiguously prohibit it: no resale or granting access to the Service ("sell,
  resell, grant access to, transfer"), no obtaining content "through any means not intentionally
  made available", no scraping/extraction, no circumventing IP blocks via VPN/proxy, no
  multi-accounting on the free tier, no powering competing AI services.

`decision` Publication requires either acceptance into Suno's partner program or a written
agreement; neither exists. The plane is internal capacity and calibration only, and this
compliance posture additionally justifies keeping every surface default-off.

`official` **All current models are scheduled for retirement**: the 2026-08-10 blog states that
when the new industry-partnership models launch "all prior models will be retired" (new ToS
2026-09-03). `decision` Model ids are a reviewed, dated list in the credential/metering crates —
never scattered constants — and an unknown id fails closed; the retirement event is handled as a
new reviewed epoch, not an edit of history.

Until the live matrix runs on an owned subscription, the GA criterion of
`PROVIDER_ONBOARDING.md` §1 is **not claimed**. The terminal state of this delivery is a verified
**preview**: everything that does not need a live subscription is proven on mock gates; the live
gates are listed in §6.

## 1. Product / plan

`official` (`suno.com/pricing` embedded plan config JSON, reviewed 2026-08-12) — exactly three
current plans; legacy `basic`/`pro_20250501` exist only as grandfathered entries:

| | Free | Pro | Premier |
|---|---|---|---|
| Monthly price | $0 | **$10/mo** | **$30/mo** |
| Annual price | — | $96/yr ($8/mo eff.) | $288/yr ($24/mo eff.) |
| Credits | 50, renew **daily** | 2,500/mo | 10,000/mo |
| Models | `v4.5-all` only | v4, v4.5, v4.5+, v5, **v5.5** | same as Pro |
| Commercial use | No | Yes | Yes |
| Queue | shared, 4 concurrent | priority, up to 10 at once | same |
| Unique | — | Personas/Voices, Custom Models (≤3), WAV | **Suno Studio** |

`official` Subscription credits do not roll over ("do not carry over from day to day or month to
month"); top-up credits never expire but require an active subscription; annual plans still drop
credits monthly. `unknown` Whether credit refresh anchors to the billing date (downloads do;
credits are not stated verbatim).

`official` Paid subscribers who exhaust their credits get **50 bonus credits/day** until renewal
(help article 2643713). `decision` The bonus drip is not saleable capacity: it is recorded as raw
quota evidence and excluded from calibrated capacity until live evidence shows its exact shape.

`official` `unknown` Top-up packs exist (`can_buy_credit_top_ups`) but pack sizes/prices sit
behind the authenticated UI — **top-up pricing is unknown** and fails closed.

`decision` The plane onboards **Pro and Premier only**. Free has no commercial rights, a daily
50-credit drip, and an explicit one-account anti-pooling clause; it is not capacity.

## 2. Credential

There is no official API credential. The working path is the subscription web session; all wire
facts in this section are `oss-hypothesis` from `github.com/gcui-art/suno-api` (3 158 stars,
LGPL-3.0, last push 2026-03-06, `src/lib/SunoApi.ts`, read 2026-08-12), corroborated in part by
`github.com/SunoAI-API/Suno-API` (stale since 2025-04-26 — schema corroboration only):

| Field | Value | Label |
|---|---|---|
| Session material | full browser `Cookie` string; the critical cookie is Clerk **`__client`** | `oss-hypothesis` |
| Session discovery | `GET https://auth.suno.com/v1/client?__clerk_api_version=2025-11-10&_clerk_js_version=5.117.0`, `Authorization: <__client value>` → `response.last_active_session_id` | `oss-hypothesis` |
| JWT mint | `POST https://auth.suno.com/v1/client/sessions/{sid}/tokens?…` → **`jwt`**, sent as `Authorization: Bearer {jwt}`; the full Cookie is re-sent and `set-cookie` merged back on every call | `oss-hypothesis` |
| Business host | `https://studio-api.prod.suno.com` | `oss-hypothesis` |
| Client impersonation | `x-suno-client`, `X-Requested-With: com.suno.android` (Android app persona) | `oss-hypothesis` |
| CAPTCHA gate | `POST /api/c/check` `{"ctype":"generation"}` → `{"required": bool}`; hCaptcha may gate generation | `oss-hypothesis` |

`unknown` JWT TTLs, Clerk version-pin sensitivity, current CAPTCHA frequency, whether the exact
hosts/paths still work today — all read from code, not from a live session. Fail closed.

`decision` **Credential shape**: the sealed envelope holds the session cookie material and the
discovered session id; short-lived JWTs are minted on demand through the same egress proxy and are
never persisted. JWT minting is idempotent session keep-alive, not a rotating refresh family — but
because a mint response may rotate the underlying Clerk token (`set-cookie` merge), the runtime
holds a **per-profile single-flight from JWT mint through envelope re-seal**, exactly the KIMI
rotating-family discipline: the winner re-seals before releasing the lock; the loser re-reads.

`decision` **Seller artifact.** The Auth Bot's Claude branch already accepts one sanctioned secret
artifact (the `sk-ant-oat01` setup-token); the Suno `__client` cookie is the same class of
artifact and is accepted the same way: a guided, one-time, no-store, bounded form; the bot never
asks for the account password, 2FA, or card data. This is a conscious, recorded deviation from the
generic "seller never sends a cookie" default — there is no other credential surface.

`decision` Stable provider subject: `unknown` whether a machine-readable user id is exposed by
the session endpoints. Until proven, the Clerk **session id** is the dedup identity; a `/me`-class
endpoint discovered live replaces it (recorded in §6).

## 3. Model admission

`official` (pricing page + help-center timeline, reviewed 2026-08-12). Current load-bearing ids:
**`v4.5-all`** (free) and **`v5.5`** (paid flagship, 2026-03-26). Paid tiers list access to
"Advanced models (v4, v4.5, v4.5+, v5, v5.5)".

| Model | Tier | Notes | Decision |
|---|---|---|---|
| `v5.5` | Pro/Premier | flagship: Voices, Custom Models, My Taste | preview, behind switch |
| `v5` | Pro/Premier | 30 s–8 min; Remaster variation control | preview, behind switch |
| `v4.5+` | Pro/Premier | Add Vocals / Add Instrumental | preview, behind switch |
| `v4.5` | Pro/Premier | 8 min in one shot | preview, behind switch |
| `v4` | Pro/Premier | Remaster, ReMi, Covers/Personas | preview, behind switch |
| `v4.5-all` | Free | free-tier only | not onboarded (Free excluded) |
| v2/v3/v3.5 | deprecated | deprecated for free users; paid availability `unknown` | fail closed |

`decision` v1 admits **song generation only** (`POST /api/generate/v2/`) across the paid model
list. Extend, Covers, Stems, Remaster, Add Vocals/Instrumental, MIDI, video/image, Studio
operations are recorded in the manifest and tariff where priced, but are **not admitted** at the
serving boundary: their per-operation credit costs are mostly unpublished (§5.1), and an unknown
price fails closed before reserve.

`official` The exact wire model id spelling (e.g. the `mv` field value `chirp-v3-5` in the OSS
client is a stale default) is `unknown` for the current picker — the reviewed id list above lives
in one place and the live matrix pins the wire spellings.

## 4. Wire

All `oss-hypothesis` (gcui-art/suno-api, read 2026-08-12), business host
`https://studio-api.prod.suno.com`:

| Operation | URL | Notes |
|---|---|---|
| Generate song | `POST /api/generate/v2/` | keys: `make_instrumental`, `mv` (model), `prompt`, `gpt_description_prompt`, `generation_type:'TEXT'`, `continue_at`, `continue_clip_id`, `tags`, `title`, `negative_tags`, `token` (hCaptcha) |
| Concat/extend | `POST /api/generate/concat/v2/` | not admitted v1 |
| Lyrics | `POST /api/generate/lyrics/` + `GET /api/generate/lyrics/{id}` | not admitted v1 |
| Stems | `POST /api/edit/stems/{song_id}` | not admitted v1 |
| Feed/status | `GET /api/feed/v2?ids=…` | poll |
| Clip | `GET /api/clip/{clipId}` | result metadata/URLs |
| CAPTCHA check | `POST /api/c/check` | `{"ctype":"generation"}` → `{"required": bool}` |
| **Quota** | `GET /api/billing/info/` | fields `total_credits_left`, `period`, `monthly_limit`, `monthly_usage` |

`decision` **Serving shape.** Suno is a task-based media API: create → poll → download. Like
Tripo3D it cannot ride a chat plane; it gets its own `ProviderMode`, two blue-green slots, and a
stable loopback origin (the KIMI deploy pattern) exposing bounded REST endpoints
(create → status → artifact). Generated audio is downloaded into our storage before delivery;
upstream media URLs are never exposed to the customer.

`decision` Generation may be gated by hCaptcha (`token` field). No CAPTCHA solving is built: if
`/api/c/check` answers `required: true`, the profile is soft-cooled and the attempt rotates; a
persistent gate is an operational state, not a customer error. This keeps us inside "no bypassing
access control" (`PROVIDER_ONBOARDING.md` §3.2).

### 4.1 Error classes

`unknown` No official error reference exists. `decision` Conservative classification until live
evidence: 401/403 after a successful JWT mint → soft auth axis (never a verdict from one
rejection); 429 → hard quota/rate wall with cooling; 5xx/timeouts pre-result → bounded transport
rotation; a finalized-but-failed generation with zero credit movement → refund the hold. The
pool-must-not-empty invariant of §8.4 applies unchanged.

## 5. Money / quota

### 5.1 Credit costs and the replacement-price decision

`official` — very few per-operation costs are published (reviewed 2026-08-12):

- **Song generation: 5 credits/song** (implied by "50 credits = 10 songs", "2 500 credits = up to
  500 songs" on the pricing page and help article 2410049). **No per-model differentiation is
  published.**
- Stems: Auto Split 50/extraction; Split from Mix 10; Advanced Split 10/stem (Premier).
- MIDI from a stem (Studio): 10. Covers: first batch free; ongoing cost unstated. Personas:
  historically 200 free then 10/song (2024, dated).
- `unknown` Costs of Extend, Remaster, Add Vocals/Instrumental, Voices, Custom Model tuning,
  video/image, and all Studio operations.

`decision` **There is no official API rate card** (no public API). The customer-facing nanoUSD
tariff is a **reviewed derived schedule**: per-credit replacement value = the worst (highest)
subscription unit economics, Pro $10 / 2 500 credits = **$0.004/credit**
(Premier is $0.003/credit; the higher value is conservative), so one song = 5 credits =
**$0.02 = 20 000 000 nanoUSD**. The schedule id is dated; if top-up pricing or an official API
price list becomes available, a new epoch replaces the derivation — history is never rewritten.
The derivation and its conservatism are recorded here and in `crates/metering/src/suno.rs`.

### 5.2 Native quota — `/api/billing/info/`

`oss-hypothesis` Fields `total_credits_left`, `period`, `monthly_limit`, `monthly_usage`.
`official` The window is **monthly** (credits refresh monthly, no rollover); the free tier's
window is daily — not onboarded.

`decision` The native window limit is **published per plan** (Pro 2 500, Premier 10 000 credits),
so — exactly as GLM — no native-capacity estimation is needed. The used fraction derives from
`monthly_usage / monthly_limit` (or `1 − total_credits_left / monthly_limit`), with measurement
resolution `ceil(SCALE / monthly_limit)`; raw counters are preserved verbatim on every
observation. `unknown` The exact semantics/freshness of the three fields (is `total_credits_left`
inclusive of top-ups? does `period` name the reset?) — fail closed until live.

### 5.3 Choosing the ledger model

`decision` Suno is **Claude-like in shape with published native limits**:

1. **API-nanoUSD ledger** — per settled generation, from the reviewed derived schedule (§5.1):
   exact per-turn replacement cost in nanoUSD.
2. **Native credits ledger** — per settled generation: the observed credit delta attributed to the
   turn when the provider reports it, else the reviewed 5-credit nominal flagged as
   schedule-derived, never as provider truth.
3. **Window observations** — `/api/billing/info/` snapshots as immutable evidence against the
   monthly window; reset evidence from `period` when present.

`capacity_nanoUSD = round_half_up(FRACTION_SCALE × ΣΔapi_nano / ΣΔused_fraction_units)` over
complete intervals, checked integer math, round-half-up — the §10.5 formula unchanged. Cohorts
merge only by exact plan (Pro/Premier) + the monthly window; a missing plan blocks aggregation.

### 5.4 Runtime ordering

`decision` Mirrors GLM §5.4: a free quota anchor right after session validation, then a periodic
poll (`CLAUDE_API_SUNO_QUOTA_POLL_SECS`); roster discovery is an independent 15-second tick. A
quota poll never runs with a pending head in the bounded turn FIFO; after the HTTP snapshot the
writer drains again, reads the durable cumulative ledgers, and completes the immutable
observation before publishing quota steering. Every provider call goes through the profile's
pinned egress proxy.

`official` risk: the ToS prohibits circumventing IP blocks via VPN/proxy. `decision` The proxy
exists to keep the seller's account geography stable (the same reason as every other plane);
deliberate geo-hopping is out of scope. This is recorded as a compliance risk, not hidden.

## 6. What remains unproven

Each `unknown` fails closed and is cleared only by a controlled live run on our own subscription:

1. Whether the OSS-documented hosts/paths (`auth.suno.com`, `studio-api.prod.suno.com`) and Clerk
   version pins work today; JWT TTLs; `set-cookie` rotation semantics.
2. A machine-readable user identity endpoint (stable subject beyond the session id).
3. Exact semantics of `total_credits_left` / `monthly_limit` / `monthly_usage` / `period`, and the
   credit reset anchor (billing date vs calendar).
4. Wire spellings of the current model ids (`mv` values) and the paid availability of
   legacy v4–v5 in the picker.
5. Per-operation credit costs beyond song/stems/MIDI (Extend, Remaster, Covers post-free-batch,
   Voices, Custom Models, video/image, Studio).
6. CAPTCHA gate frequency and whether generation is served at all without solving it.
7. Behavior at quota exhaustion (the 50 bonus credits/day drip shape) and at plan downgrade.
8. The new ToS text effective 2026-09-03 and the scope of the announced model retirement.
9. Top-up pack pricing (the only candidate for an official per-credit money anchor).
10. Partner-program status (legal gate; blocks publication, not the backend).

## 7. Auth Bot: acquisition flow (decision)

`decision` A separate `HandoffKind::Suno`, steps `su_proxy → su_ready → su_wait`, button
`su:ready`, modeled on the GLM static-key branch with the Claude setup-token artifact discipline:

1. `su_proxy`: proxy as text (canonicalization via `suno_credential::normalize_proxy_url`). There
   is one platform (`suno.com`), no region fork.
2. The seller receives the newcomer guide: set up the proxy first, do not change profile/IP,
   activate **exactly** the plan of the offer product (Pro or Premier — the two products in the
   catalog), then extract the session cookie: the guide walks the seller through copying the
   `__client` cookie from their own browser session (one-time, bounded form, no-store).
3. `su_ready`: the seller confirms; the bot asks for the cookie artifact.
4. `su_wait`: the bot validates through the pinned proxy: Clerk session discovery → JWT mint →
   free quota probe (`/api/billing/info/`) → plan corroboration (observed `monthly_limit` 2 500 /
   10 000 vs the declared plan; mismatch → back to `su_ready`) → the admission micro-smoke (a
   single 5-credit song = $0.02 derived cost exceeds the default $0.0001 admission cap, so the
   cap must be explicitly raised by the operator for Suno admission, or admission stops at the
   free probe; recorded as an open admission-budget question, fail closed) → seal envelope →
   atomic roster publish → payout completion.
5. Cancel/retry/expiry/wrong-plan leave neither a credential file nor a roster row and do not
   complete the payout.

## 8. Delivery status

| Stage | Artifact | Status |
|---|---|---|
| research / capability manifest | this file | done |
| reviewed derived rate card | `crates/metering/src/suno.rs` | done (8e2d147d) |
| calibration authority (schema 0050) | `crates/registry/migrations_pg/0050_suno_window_calibration.sql` | done (this commit) |
| pricing provider admission (schema 0052) | `crates/registry/migrations_pg/0052_suno_pricing_provider.sql` | done (this commit) |
| observation types | `crates/registry/src/suno_calibration.rs` | done (this commit) |
| credential | `crates/suno-credential` | done (this commit) |
| calibration estimator | `crates/forward/src/suno_calibration.rs` | done (this commit) |
| Auth Bot: validation protocol + roster | `crates/authbot/src/{suno_session,suno_roster}.rs` (+`main.rs` env) | done (this commit) |
| Auth Bot: seller wizard | `crates/authbot/src/bot.rs` (+`db.rs` recovery, `main.rs`) | pending |
| runtime primitives + gateway | `crates/forward/src/suno/**` | pending |
| server wiring | `crates/server/src/{config,main,poller,http}.rs` | pending |
| observability / admin projection | `observability/**`, `docs/ops/MONITORING.md`, `GET /suno-subs` | pending |
| admin UI consumer | `apps/admin` | pending |
| safe live-runner | `tools/suno_calibration/`, `docs/ops/SUNO_CALIBRATION.md` | pending |
| production activation boundary | systemd/Caddy — default-off pins only | pending (dormant) |
| live matrix on our subscription | — | **subscription needed (blocked on a human)** |

Queue and SHA tracking — `research/SUNO_PLANE_PROGRESS.md`.

## 9. Sources

All links reviewed 2026-08-12.

- `https://suno.com/pricing` — plan ladder, credits, model list (embedded plan config JSON)
- `https://suno.com/terms` — ToS, last revision 2026-03-26 (anti-resale/automation/proxy clauses)
- `https://suno.com/blog/suno-updates-tos` (2026-08-10) — new ToS 2026-09-03, model retirement
- `https://suno.com/blog/{v4,introducing-v4-5,v5-5,stem-separation-updates,personas}`
- help.suno.com articles: 2417089 (credits), 2410049 (plan types), 5782721 + 2409473 (model
  timeline), 8105153 (v5), 7986753 (v2/v3 deprecation), 12702337 (stems), 8128193 (Studio MIDI),
  2872257 (covers), 2643713 (bonus credits), 9601665 (annual credits), 13614785 (downloads),
  2746945 (ownership), 2479873 (WAV)
- `https://platform.suno.com` — partner-gated API platform (login wall on every docs path)
- `github.com/gcui-art/suno-api` @ master (last push 2026-03-06), LGPL-3.0 — session/quota wire
  blueprint (read-only research via the web; no clone executed)
- `github.com/SunoAI-API/Suno-API` (stale 2025-04-26), MIT — schema corroboration only
