# Tripo3D (VAST / Holymolly) — provider capability manifest

Integration status: **research complete; default-off backend preview ahead; nothing is published**
(no production defaults, no public catalog, no router presets, no storefront, no public docs).
Source review date — **2026-08-12**.

This document was created per `docs/engine/PROVIDER_ONBOARDING.md` §3.3 and is the capability
manifest of the Tripo3D plane. Every claim is labeled per the evidence hierarchy from §3.1:
`official`, `live`, `oss-hypothesis`, `decision`, `unknown`, `not-applicable`. The mechanical edit
map is `docs/engine/PROVIDER_WIRING_CHECKLIST.md`; the closest reference implementations are
`docs/engine/GLM_PROVIDER.md` (static-API-key acquisition) and the Codex image lane
(`crates/forward/src/codex/images.rs`) for a media-generation, non-chat serving surface.

## 0. Scope and intentional limitations

The Tripo3D plane is built **backend-only**, per the product owner's frame: engine runtime,
metering, calibration, Auth Bot, admin control room — and **no publication** to the public
catalog, router presets, commerce/OpenKeys pricing, the site, or client docs. The decision
mirrors GLM §0 and is additionally forced by the provider's own terms:

- `official` Terms of User Agreement (`www.tripo3d.ai/terms`, last updated 2025-07-11):
  §3.2 forbids making the service "available to any third party, including end users, without
  the prior written authorization and consent of Holymolly", forbids buying/selling/transferring
  API keys without prior written approval; §2.2 forbids more than one account per platform.
- `decision` Pooling/resale therefore requires a written agreement with Holymolly
  (`business@vastai3d.com`). Until it exists, the plane is internal capacity and calibration
  only; any publication is a separate legal review and is not performed in this work.

Until the live matrix runs on an owned account, the GA criterion of `PROVIDER_ONBOARDING.md` §1
is **not claimed**. The terminal state of this delivery is a verified **preview**: everything
that does not need a live account is proven on mock gates; the live gates are listed in §6.

## 1. Product / plan

`official` Tripo3D operates **two independent billing systems** (support FAQ Q11): the consumer
web app ("Tripo Studio", subscriptions) and the API platform (`platform.tripo3d.ai`, prepaid
credits). Credits are not shared between them, and pricing differs.

| Plane | Purpose | Base URL | Billing |
|---|---|---|---|
| Tripo Studio (web) | consumer 3D app | `https://www.tripo3d.ai` | subscriptions (Free/Pro/Max/Team) |
| Tripo3D API platform | developer API | `https://api.tripo3d.ai/v2/openapi` (global), `https://api.tripo3d.com/v2/openapi` (CN) | prepaid credits, $0.01/credit |

`decision` Our provider is the **API platform only**. Pooled capacity is accounts with topped-up
API credit balances; Studio subscriptions do not yield API capacity and are out of scope.
The Studio plan ladder (Free 200 cr/mo, Pro 3 000 cr/mo $19.90, Max 25 000 cr/mo $89.90,
Team 45 000 cr/mo/seat $109.90 — `www.tripo3d.ai/pricing`, reviewed 2026-08-12) is recorded for
completeness only.

### 1.1 API billing

`official` (`platform.tripo3d.ai/docs/other/billing.md`, changelog to 2026-06-03;
`docs.tripo3d.ai/get-started/pricing.html`):

- Prepaid only ("pay-before-you-go"); **$1.00 = 100 credits ($0.01/credit)**, top-ups in units
  of 100 credits. Purchased credits never expire; no refunds; Stripe processes payments.
- Free trial: 300 credits, valid 2 weeks.
- `unknown` Volume/bundle discounts — none published; the purchase screen is login-gated.

`decision` There is no subscription "plan" on the API side, so there is no machine-readable plan
identity and no plan ladder to encode. The Auth Bot product is a **topped-up API account with a
declared top-up cohort** (see §7); the authoritative account state is the balance endpoint (§5.2).

## 2. Credential

`official` (`docs.tripo3d.ai/get-started/quick-start.html`, support FAQ):

| Field | Value | Label |
|---|---|---|
| Scheme | `Authorization: Bearer <API_KEY>` on every request | `official` |
| Key issuance | console `https://platform.tripo3d.ai/api-keys` | `official` |
| Key format | prefix `tsk_` (a `tcli_` Client ID exists but is not a credential — 401) | `official` |
| Scopes | none documented; keys are all-or-nothing per account | `official` |
| Refresh/expiry | none documented — static key, rotation = reissue in console | `official` |
| Task isolation | **a task is queryable only by the exact key that created it** (404 otherwise) | `official` |

`decision` The acquisition model follows the GLM static-key branch: the seller registers their own
platform account, tops up the declared amount, creates an API key, and sends the key to the bot.
The bot validates it (free balance probe §5.2, then the admission micro-smoke §7), seals the AEAD
envelope, and atomically publishes the roster before completing the payout. The seller never sends
a password, 2FA, cookies, or card data; the API key is the only credential artifact.

`decision` Stable provider subject: there is no documented `/me` identity endpoint. Like GLM, the
key itself is the dedup identity — the raw key never leaves the envelope; profile equality is
compared on opened envelopes by the Auth Bot.

`decision` Task-per-key isolation is load-bearing for the pool design: a task's whole lifecycle
(create → poll → result download) is pinned to the creating profile. Sticky affinity is by task,
not by conversation.

`decision` The base URL is pinned per profile from an allowlist of exactly two origins
(`https://api.tripo3d.ai` global, `https://api.tripo3d.com` CN); the undocumented `apiv3` host
(`https://openapi.tripo3d.ai`, mesh segmentation v2) is `unknown` and not used in v1.

## 3. Model admission

`official` (`docs.tripo3d.ai` per-task pages, reviewed 2026-08-12). Generation tasks accept these
`model_version` values (omission defaults to `v2.5-20250123`):

| Task type | Model versions | Notes |
|---|---|---|
| `text_to_model`, `image_to_model` | `v3.1-20260211`, `v3.0-20250812`, `P1-20260311`, `v2.5-20250123`, `v2.0-20240919`, `Turbo-v1.0-20250506`, legacy `v1.4-20240625` | P1 = low-poly smart mesh; face caps v3.1 1.5M/2M, v3.0 1M/1.5M tris |
| `multiview_to_model` | same minus Turbo/v1.4 | exactly 4 ordered views, front mandatory |
| `texture_model` | `v3.0-20250812`, `v2.5-20250123` | requires `original_model_task_id` |
| `mesh_segmentation` | `v1.0-20250506` (v2 API); `v2.0-20260430` (apiv3, unused) | |
| `mesh_completion` | `v1.0-20250506` (overview also lists `P-v2.0-20251225`) | |
| `highpoly_to_lowpoly` | `P-v2.0-20251225` docs vs `P-v2.0-20251226` SDK — **conflict, unknown** | probe before wiring |
| `animate_prerigcheck` | `v2.0-20250506`, `v1.0-20240301` | free |
| `animate_rig` | `v2.5-20260210`, `v2.0-20250506`, `v1.0-20240301` | |
| `animate_retarget`, `convert_model`, `import_model` | version-independent | 16 animation presets; formats GLTF/USDZ/FBX/OBJ/STL/3MF |
| `text_to_image`, `generate_image` | `flux.1_kontext_pro` (default), `flux.1_dev`, `gpt_4o`, `gpt_image_1.5`, `gpt_image_2`, `midjourney`, `gemini_2.5_flash_image_preview`, `gemini_3_pro_image_preview`, `gemini_3.1_flash_image_preview` | |
| `generate_multiview_image`, `edit_multiview_image` | — | concurrency 1 |

`decision` v1 of the plane admits the 3D core only: `text_to_model`, `image_to_model`,
`multiview_to_model`, `texture_model` — across the reviewed `model_version` sets above. Image
generation (`text_to_image`/`generate_image`), animation, segmentation/completion, convert/import
are recorded in the tariff (they have official prices) but are **not admitted** at the serving
boundary until their live gates run; unknown or unlisted `model_version` fails closed at reserve.

`decision` Served-vs-requested: Tripo3D does not silently re-route models (no evidence of it), but
the immutable turn event still records the requested `model_version` and the task's resolved
identity separately, and money follows the exact `consumed_credit` the provider reports (§5.1),
so a silent re-route cannot misprice — it can only fail the admission check.

## 4. Wire

`official` (`docs.tripo3d.ai` quick-start / file-upload / task-query pages; envelope per the
errors page):

| Operation | URL | Headers | Body | Result |
|---|---|---|---|---|
| Create task | `POST {base}/v2/openapi/task` | `Authorization: Bearer tsk_…` | JSON, discriminator `"type"` | `{"code":0,"data":{"task_id"}}` |
| Poll task | `GET {base}/v2/openapi/task/{task_id}` | same | — | task object (below) |
| Upload image | `POST {base}/v2/openapi/upload/sts` | same | multipart, ≤20 MB | `image_token` |
| STS model upload | `POST {base}/v2/openapi/upload/sts/token` | same | — | temporary S3 credentials |
| Balance | `GET {base}/v2/openapi/user/balance` | same | — | §5.2 |

- Envelope: success `{"code": 0, "data": …}`; errors `{"code": int, "message", "suggestion"}` +
  HTTP status; every response carries `X-Tripo-Trace-ID`.
- Task object: `task_id, type, status, input, output, progress (0–100), consumed_credit,
  queuing_num, running_left_time, create_time`; failed adds `error_code`.
- `status`: ongoing `queued`/`running`; finalized `success`/`failed`/`banned`/`expired`/
  `cancelled`/`unknown`.
- `official` **No webhooks**: callbacks were removed in changelog v1.1.0 — polling only.

`decision` **Serving shape.** Tripo3D is a task-based media API, not a chat protocol; it cannot
ride the Anthropic Messages plane like KIMI/GLM. The plane gets its own `ProviderMode`, two
blue-green slots and a stable loopback origin (the KIMI deploy pattern), exposing bounded REST
endpoints (create → status → artifact download). Long generation means the plane holds the
upstream task lifecycle itself: reserve at admission, create upstream task, poll, download the
artifact to our storage before the signed URL expires (§5.3), then settle exactly.

### 4.1 Error classes

`official` (errors page, rate-limits page):

| Status / code | Meaning | Our reaction |
|---|---|---|
| HTTP 429 code `2000` (+`Retry-After`) | concurrency/rate limit exceeded | hard provider wall → cooling per parsed `Retry-After`, rotate |
| HTTP 429 code `1007` | generic rate limit | bounded transport rotation |
| HTTP 403 code `2010` | insufficient balance at task creation | quota wall — provider verdict; profile out until balance probe shows funds |
| HTTP 401 | invalid key (`tcli_` misuse documented) | auth axis: soft cooling + probe; never a verdict from one 401 |
| task `failed`/`expired` | credits refunded (`consumed_credit` = 0) | settle to zero, refund the hold |

`decision` The pool-must-not-empty invariant (`PROVIDER_ONBOARDING.md` §8.4) applies unchanged:
only hard provider verdicts (429 with `Retry-After`, 403 `2010`) rest a profile; transport and
auth inferences are a soft axis that can never deny admission on its own.

## 5. Money / quota

### 5.1 Official rate card (replacement cost)

`official` (`billing.md`, authoritative over the partially stale pricing page; reviewed
2026-08-12). Base costs in credits; **1 credit = $0.01 = 10 000 000 nanoUSD**:

| Task | P1 (`P1-20260311`) | Turbo / v3.1 / v3.0 / v2.5 / v2.0 | v1.4 legacy |
|---|---|---|---|
| `text_to_model` | 30 no-texture / 40 std-texture | 10 / 20 | 20 |
| `image_to_model` | 40 / 50 | 20 / 30 | 30 |
| `multiview_to_model` | 40 / 50 | 20 / 30 | — |
| `refine_model` (legacy) | — | — | 30 |

Surcharges (stack on base, H2/H3 only; P1 is all-in): `smart_low_poly` +10 · `generate_parts`
+20 · `quad` +5 · style +5 · `texture_quality` standard +10 / detailed +20 / extreme +30 ·
`geometry_quality=detailed` +20. Std-texture base price includes PBR.

Other tasks: texture standard 10 / detailed 20 / extreme 30 (+5 style ref) · segmentation 40 ·
part completion 50 · post-stylization 20 · post low-poly 30 · convert basic 5 / advanced 10 ·
rig check **free** · rigging 25 · animation retarget 10 per animation · import free ·
`text_to_image` 5 · `generate_image` 5 or 10 · `generate_multiview_image` 10 ·
`edit_multiview_image` 5 per edited image.

`unknown` A second official surface (`developers.tripo3d.ai/en/pricing`) adds rows (Retopology
v2/v1 30/10, Smart Segmentation 85/55, Part Completion Quick Cap 30, per-resolution image
pricing) that are not confirmed billable via the public API — not admitted, fail closed.

`decision` The per-turn money authority is the provider-reported **`consumed_credit`** of the
finished task — an authoritative native consumption per turn. The tariff table above prices the
**reserve** (conservative hold = worst case of base + selected surcharges) and cross-checks
settlement: a `consumed_credit` that exceeds the tariff's maximum for the admitted task shape is a
typed anomaly (quarantine), never silent acceptance.

### 5.2 Native quota — the balance endpoint

`oss-hypothesis` `GET /v2/openapi/user/balance` → `data: {"balance": float, "frozen": float}`
(official Python SDK `client.py`/`models.py` @ `4115894a0a603c5183c9ed6dc8662745562c8941`, MIT;
the endpoint exists only as a changelog mention, not a docs page).

`unknown` The unit of `balance`/`frozen` (credits vs dollars) and their exact decimal semantics
are not doc-verified. Per the onboarding skill, the values are preserved as **raw quota evidence**
(floats are parsed into fixed-point strings, never binary float money) until a live run proves the
unit; nothing is divided to invent capacity.

### 5.3 Choosing the ledger model

`decision` Tripo3D is a **dual-ledger provider with per-turn native consumption** (GPT-like in
shape), with one simplification: the native unit (credits) and the official API dollar price are
linked by a published fixed rate ($0.01/credit), so the API-dollar leg is exact, not estimated:

1. **API-nanoUSD ledger** — `consumed_credit × 10 000 000` nanoUSD per settled turn.
2. **Native credits ledger** — `consumed_credit` (millicredits, credits × 1e3, to keep any
   fractional credit exact in integers), from the same authoritative field.
3. **Balance observations** — `balance`/`frozen` snapshots as immutable raw evidence; there is no
   quota *window*: prepaid balance has no reset. Calibration therefore answers not "how much fits
   in the window" but "how much sellable capacity remains": `balance − frozen`, exact once the
   unit is proven, `null` until then.

`decision` No window estimator in the KIMI/GLM sense: there is no duration, no reset, no rolling
semantics. The calibration schema keeps the same four-table shape (turn events, subject spend,
observations, calibration rows) but the "window" degenerates to a single account-balance track
(`window_duration_secs = 0` is rejected by the schema — the balance track uses its own sentinel
documented in the migration). Cohorts aggregate by declared top-up cohort only.

### 5.4 Runtime ordering

`decision` Mirrors GLM §5.4 adapted to polling: the first free balance anchor after key
validation, then a periodic poll (`CLAUDE_API_TRIPO3D_BALANCE_POLL_SECS`); roster discovery is an
independent 15-second tick. A balance poll never runs with a pending head in the bounded turn
FIFO; after the HTTP snapshot the writer drains the FIFO again, reads the durable cumulative
ledgers, and completes the immutable observation before publishing capacity steering.

`official` Result URLs expire quickly — official sources conflict (5 minutes on the task-query
page vs 60 seconds in FAQ Q10). `decision` Treat TTL as ≤60 s: the plane downloads artifacts into
its own storage immediately on task success and serves its own URLs; customer-facing delivery
never exposes the upstream signed URL.

## 6. What remains unproven

Each `unknown` fails closed and is cleared only by a controlled live run on our own account:

1. The exact schema and unit of `GET /user/balance` (`balance`/`frozen`: credits or dollars,
   float semantics).
2. `consumed_credit` precision (integer vs fractional credits) and its presence on every
   finalized task.
3. Task `expired` duration (undocumented; the FAQ answer is a literal "XXX" placeholder).
4. Result URL TTL (5 min vs 60 s conflict) — handled conservatively, still needs live pin.
5. Refund semantics for `cancelled`/`banned` tasks (only `failed`/`expired` are documented).
6. The `highpoly_to_lowpoly` version string conflict (docs `P-v2.0-20251225` vs SDK
   `P-v2.0-20251226`).
7. Real concurrency enforcement vs the documented per-account limits (non-P1 10, P1 5, etc.).
8. Whether `texture_quality=extreme` (+30) is accepted on the public API (billing changelog only).
9. Top-up bundle pricing (only flat $0.01/credit is published).
10. Written-consent status for pooling (legal gate; blocks publication, not the backend).

## 7. Auth Bot: acquisition flow (decision)

`decision` A separate `HandoffKind::Tripo3d`, steps `t3_proxy → t3_ready → t3_wait`, button
`t3:ready`, modeled on the GLM static-key branch:

1. `t3_proxy`: proxy as text (canonicalization via `tripo3d_credential::normalize_proxy_url`),
   platform choice (global `api.tripo3d.ai` / CN `api.tripo3d.com`) by button.
2. The seller receives the newcomer guide: set up the proxy before opening the account, do not
   change profile/IP, register on `platform.tripo3d.ai`, top up **exactly the declared amount**
   of the offer product (the product catalog entry names the top-up cohort, e.g. "Tripo3D API
   $50"), create an API key at `/api-keys`.
3. `t3_ready`: the seller confirms readiness; the bot asks for the API key.
4. `t3_wait`: the bot validates: free balance probe (§5.2) → admission micro-smoke (one minimal
   paid task under the aggregate cap $0.0001 per the AGENTS.md admission rule — a `text_to_image`
   basic task at 5 credits = $0.05 exceeds the cap, so admission uses the free
   `animate_prerigcheck`-style zero-cost probe if a live account confirms one exists, otherwise
   the cap must be explicitly raised by the operator; recorded as an open admission-budget
   question, fail closed) → seal envelope → atomic roster publish → payout completion.
5. Cancel/retry/expiry/wrong-cohort leave neither a credential file nor a roster row and do not
   complete the payout. An invalid key (401), a balance contradicting the declared cohort beyond
   tolerance, or a foreign base host — return to `t3_ready` with a safe hint.

## 8. Delivery status

| Stage | Artifact | Status |
|---|---|---|
| research / capability manifest | this file | done |
| official rate card | `crates/metering/src/tripo3d.rs` | done (2f80d806) |
| calibration authority (schema 0049) | `crates/registry/migrations_pg/0049_tripo3d_calibration.sql` | done (this commit) |
| observation types | `crates/registry/src/tripo3d_calibration.rs` | done (this commit) |
| credential | `crates/tripo3d-credential` | pending |
| calibration estimator | `crates/forward/src/tripo3d_calibration.rs` | pending |
| Auth Bot protocol + wizard | `crates/authbot/src/{tripo3d_key,tripo3d_roster}.rs`, `bot.rs` | pending |
| runtime primitives + gateway | `crates/forward/src/tripo3d/**` | pending |
| server wiring | `crates/server/src/{config,main,poller,http}.rs` | pending |
| observability / admin projection | `observability/**`, `docs/ops/MONITORING.md`, `GET /tripo3d-subs` | pending |
| admin UI consumer | `apps/admin` | pending |
| safe live-runner | `tools/tripo3d_calibration/`, `docs/ops/TRIPO3D_CALIBRATION.md` | pending |
| production activation boundary | systemd/Caddy — default-off pins only | pending (dormant) |
| live matrix on our account | — | **account needed (blocked on a human)** |

Queue and SHA tracking — `research/TRIPO3D_PLANE_PROGRESS.md`.

## 9. Sources

All links reviewed 2026-08-12.

- `https://docs.tripo3d.ai/get-started/{introduction,quick-start,pricing,changelog,rate-limits}.html`
- `https://docs.tripo3d.ai/task-query/get-your-task-result.html`
- `https://docs.tripo3d.ai/other/support-faq.html`
- `https://platform.tripo3d.ai/docs/other/{billing.md,faq.md}` (authoritative billing surface)
- `https://www.tripo3d.ai/pricing` — Studio plan ladder
- `https://www.tripo3d.ai/terms` — Terms of User Agreement (2025-07-11): §2.1(b), §2.2, §3.2, §5.2, §12.3
- `https://www.tripo3d.ai/blog/announcement-tripo-openapi-platform-payment-system` — $0.01/credit
- `github.com/VAST-AI-Research/tripo-python-sdk` @ `4115894a0a603c5183c9ed6dc8662745562c8941`, MIT
  (read-only research via the web; no clone executed)
