# GLM (Zhipu AI / Z.ai) — provider capability manifest

Integration status: **default-off backend preview, research/runtime/server composition complete;
the public production units are pinned disabled, while the first owned subscription, live matrix,
and a compliant private serving boundary remain ahead**.
Source review date — **2026-08-03**.

This document was created per `docs/engine/PROVIDER_ONBOARDING.md` §3.3 and is the capability
manifest of the GLM plane. Every claim is labeled per the evidence hierarchy from §3.1:
`official`, `live`, `oss-hypothesis`, `decision`, `unknown`, `not-applicable`. The mechanical
edit map is `docs/engine/PROVIDER_WIRING_CHECKLIST.md`; the reference implementation is
`docs/engine/KIMI_PROVIDER.md` (KIMI is the closest analogue: a Chinese subscription provider
with an Anthropic-compatible transport).

## 0. Scope and intentional limitations

The GLM plane is built **backend-only**: engine runtime, metering, calibration, Auth Bot,
and the internal live-runner. The provider is **not published** to the public catalog, the router's
`/v1/models`, commerce/OpenKeys pricing, the site, or client documentation — the decision
mirrors KIMI §0.

`decision` The implemented dispatcher is embedded in the Anthropic Messages process, whose stable
origin is customer-facing. Merely omitting GLM from discovery therefore does not make an enabled
runtime private: a customer who knows an exact alias could still call it. The current
`claude-api.service`, `claude-api@.service`, and `claude-api-anthropic@.service` definitions pin
`CLAUDE_API_GLM_ENABLED=0` at argv level, overriding any staged shared-env value. Removing that pin
requires both the owned-subscription live matrix and either a genuinely private serving boundary or
written permission that resolves the terms conflict below; a hidden public alias is not an
acceptable preview.

- `official` `docs.z.ai/legal-agreement/subscription-terms` **explicitly forbids** resale:
  "you may not resell, sub-resell, repackage, aggregate, proxy or otherwise provide the GLM
  Coding Plan to any third party"; "general-purpose API access" from one's own applications/SaaS
  is forbidden without a separate written agreement; the subscription is tied to a single
  individual. Sanctions — quota reduction, suspension/termination, ban after >3 violations
  (`docs.z.ai/devpack/usage-policy`).
- `decision` It is precisely this prohibition, as with KIMI ("personal interactive use only"),
  that finally fixes the backend-only mode: the integration is needed for internal capacity and
  calibration; any publication requires a separate legal review and is not performed in this work.
- `decision` The reseller business model (audit of the request): Chinese 中转站 (relay stations,
  i.e. new-api/one-api relays) buy a Coding Plan, take a key from the console, register it as a
  channel, and sell Anthropic/OpenAI-compatible access on top of the quota — exactly the model of
  our Claude pool (`oss-hypothesis`: new-api#2051 added `open.bigmodel.cn/api/coding/paas/v4`
  precisely for this).

Until publication happens, the GA criterion of §1 `PROVIDER_ONBOARDING.md` is **not claimed**. The
terminal state is a verified **preview**: everything that does not require a live subscription is
proven on mock gates; the live gates are listed in §6 and await our own subscription.

## 1. Product / plan

`official` Zhipu AI (Z.ai / open.bigmodel.cn) separates independent access systems:

| Plane | Purpose | Base URL | Billing |
|---|---|---|---|
| Z.ai Open Platform | pay-per-token developer API | `https://api.z.ai/api/paas/v4` (int.), `https://open.bigmodel.cn/api/paas/v4` (CN) | per token |
| GLM Coding Plan (subscription) | subscription coding plan | `https://api.z.ai/api/anthropic` + `https://api.z.ai/api/coding/paas/v4` (int.), `https://open.bigmodel.cn/api/anthropic` + `…/api/coding/paas/v4` (CN) | from subscription quota |
| Z.ai web/app chat | consumer chat | — | subscription, **provides no API** |

`official` A wrong endpoint = plan quota is not consumed and the call is not served
(devpack/faq, error "1113 Insufficient Balance" when calling outside plan endpoints).
`official` A Coding Plan key is tied to the product type: a Team Plan Key is "not interchangeable
with other Z.AI's API Keys"; error `1315` — "API Key is limited to enterprise coding package
scenarios".

`decision` Our provider is **GLM Coding Plan only, individual credits plans only**
(Lite/Pro/Max of the new system, see §1.1). Open Platform is exclusively the authority for
official pricing (replacement cost). Team Plan (a different quota unit — tokens, not credits) and
legacy prompts plans (V1/V2, discontinued from sale 2026-07-30) are **not supported** in the
first version of the plane and fail closed.

### 1.1 Pricing plans (credits system, since 2026-07-30)

`official` (`docs.z.ai/devpack/overview`, `docs.z.ai/devpack/notice/usage-revision`; the Chinese
version `docs.bigmodel.cn/cn/coding-plan/overview` agrees):

| Plan | Credits / 5h | Credits / week |
|---|---|---|
| Lite | 2 000 | 10 000 |
| Pro | 12 000 | 60 000 |
| Max | 28 000 | 140 000 |

- Charge formula: `credits = (input_tokens × in_mult + cached_input_tokens × cache_mult +
  output_tokens × out_mult) / 10 000`. Multipliers — §5.2.
- **Off-peak −50 %**: peak = Mon–Fri 14:00–18:00 SGT (UTC+8); outside peak the charge is halved.
- Reset: 5-hour credits — dynamically ("5 hours after consumption", rolling);
  weekly — every 7 days from the order moment.
- When quota is exhausted, the account balance is **not charged**; calls outside the plan are
  impossible.
- `unknown` Current USD plan prices are not pinned by a provider-owned page (z.ai/subscribe is
  JS-rendered). Community: Lite ~$18/mo, Pro ~$72–80/mo, Max ~$160–168/mo. The subscription price
  does not participate in calculations (calibration computes API replacement cost, not payback).

`official` Legacy prompts plans (V1/V2) live on with old subscribers until the end of the billing
cycle: Lite ~80 prompts/5h, Pro ~400/5h, Max ~1 600/5h; GLM-5.2/5-Turbo burn 3× at peak.
`decision` Legacy is not onboarded: the Auth Bot accepts only credits plans; quota observations
incompatible with credits semantics fail closed (see §5.3).

`official` Team Plan: Standard 60M tokens/5h + 300M/week, Premium 160M/5h + 800M/week — a
**different unit** (tokens, not credits). `decision` Not supported in v1; a Team key is recognized
by mismatch with the expected credits form and rejected at onboarding.

## 2. Credential

`official` Unlike KIMI (OAuth device flow), the GLM Coding Plan works with a **static API key**
from the console: Individual Coding Plan → Plan Overview → create an API Key
(`docs.z.ai/devpack/quick-start`). There is no refresh family; rotation = reissuing the key in
the console.

`decision` The acquisition model follows the Auth Bot's Claude setup-token branch: the seller goes
through the proxy/account guide, buys the exact plan on their own Z.ai/bigmodel.cn account,
creates a key in the console, and sends the key to the bot. The bot validates it (§7), seals the
AEAD envelope, and atomically publishes the roster before completing the payout. The seller never
sends a password, 2FA, cookies, or a card; the API key is the only credential artifact, like
`sk-ant-oat01-…` for Claude.

`decision` The base URL is stored in the sealed credential **per profile**: an int seller →
`https://api.z.ai`, a CN seller → `https://open.bigmodel.cn`; an allowlist of exactly two hosts.
Int/CN keys are incompatible between the platforms (decision from the key's binding to the
issuing console).

### 2.1 Identity and key validation

`oss-hypothesis` A machine-readable identity endpoint (the analogue of KIMI's `/me`) was **not
found** in either the documentation or OSS. The validity-check role is performed by the quota
endpoint (§5.2): `GET {base}/api/monitor/usage/quota/limit`, header `Authorization: <key>`
**without the Bearer prefix**; an invalid key returns **HTTP 200 with `code: 401` in the body**
(onWatch `zai_client.go`/`zai_types.go`, pinned SHA — §8).

`decision` Stable provider subject: the quota is tied to the account/key; in the absence of `/me`,
the key itself serves as the subject identity — more precisely, its keyed-BLAKE3 digest (the raw
key never leaves the envelope). Dedup rule: the same key cannot occupy two profiles; republishing
the same digest replaces the profile in place (as with KIMI by subject).

`decision` Authoritative paid plan identity: a machine-readable `user_level_name` does not exist
(`unknown`). The plan is recorded **declaratively from the offer product** (the operator creates
an offer "GLM Coding Plan Pro", the seller must activate exactly it) and is **corroborated** by the
quota endpoint: the published window limits (2 000/12 000/28 000 credits per 5h) map unambiguously
to Lite/Pro/Max. An observed limit contradicting the declared plan — fail closed, the profile is
out of rotation pending operator review.

## 3. Model admission

`official` On the Coding Plan **only three models** can be called (devpack/faq: "Only the
following three models can be called"; devpack/overview):

| Subscription model | Official model (rate card) | Context | Max output | Tier | Non-stream | Incremental stream | Usage | Quota | Decision |
|---|---|---|---|---|---|---|---|---|---|
| `glm-5.2` | `glm-5.2` | 1 000 000 | 131 072 | all plans | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |
| `glm-5-turbo` | `glm-5-turbo` | 200 000 | 131 072 | all plans | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |
| `glm-4.7` | `glm-4.7` | 200 000 | 131 072 | all plans | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |

`official` **Served ≠ requested**: «调用历史模型 GLM-5.1/GLM-5 都将自动切换至 GLM-5.2» (Chinese:
"calls to the legacy models GLM-5.1/GLM-5 are automatically switched to GLM-5.2") — requests to
glm-5.1/glm-5 are silently routed to glm-5.2 (Chinese overview). `decision` Metering runs against
the **served** model from the response (the same trap as KIMI §3): the immutable turn event stores
requested and served separately; a missing served model in the response — billing fails closed at
reserve.

`official` The special syntax `glm-5.2[1m]` in the official Claude Code mapping
(`ANTHROPIC_DEFAULT_SONNET_MODEL: glm-5.2[1m]`) is a 1M-window selector, not a separate model.
`decision` The canonical id is `glm-5.2`; the bracket form is accepted as an alias (like `k3[1m]`).

`official` Thinking: `thinking.type` enabled/disabled; `reasoning_effort` (GLM-5.2 only:
max/xhigh/high/medium/low/minimal/none; low/medium→high, xhigh→max; none/minimal — thinking off).
`unknown` Whether disabling thinking changes the served model/tariff (for KIMI it did) — until
live, fail closed: billing by served model from the response; on mismatch with the allowed set —
an error.

`oss-hypothesis` Vision (`glm-4.6v`) and the highspeed variant `glm-5.2-highspeed` appear in
models.dev for `zai-coding-plan`, but are not part of the official trio of text models.
`decision` Not published in v1: the capability is recorded as unavailable, and no budget is spent.

## 4. Wire

| Operation | URL | Headers | Body | Framing | Usage | Errors |
|---|---|---|---|---|---|---|
| Generation (Anthropic) | `POST {base}/api/anthropic/v1/messages` | `Authorization: Bearer` (official, Claude Code via `ANTHROPIC_AUTH_TOKEN`; wire confirmation `oss-hypothesis` gsd-2#3874) | Anthropic Messages | SSE | `unknown` (cache fields) | §4.2 |
| Generation (OpenAI) | `POST {base}/api/coding/paas/v4/chat/completions` | `Authorization: Bearer` (official) | OpenAI Chat | SSE | `official` `prompt_tokens/completion_tokens/prompt_tokens_details.cached_tokens/total_tokens` | §4.2 |
| Quota | `GET {base}/api/monitor/usage/quota/limit` | `Authorization: <key>` **without Bearer** (`oss-hypothesis`) | — | JSON | — | HTTP 200 + `code:401` in body |
| Catalogue | `GET {base}/api/anthropic/v1/models` | `oss-hypothesis` | — | JSON | — | `unknown` |

`decision` **The plane serves only the Anthropic route** — as with KIMI, this allows reusing the
engine's native Anthropic path without a translation layer on the scale of `gemini/`. The
OpenAI route is documented but not stood up in v1.

`oss-hypothesis` Z.ai's risk control detects "SDK-based access" — requests without identifying
tool headers — and applies throttling/bans (pi#4187). `decision` Generation traffic carries the
**full Claude Code fleet fingerprint, identical to the Claude plane and fed from the SAME
shared env** (`CLAUDE_API_UA` (+ pool via `|`), `CLAUDE_API_BETA`, `CLAUDE_API_ANTHROPIC_VERSION`,
`CLAUDE_API_X_APP`, `CLAUDE_API_SL_*`, `CLAUDE_API_CC_VERSION`, `CLAUDE_API_CC_ENTRYPOINT`,
`CLAUDE_API_IDENTITY`, `CLAUDE_API_INJECT_BILLING`; auto-refreshed by a live client —
`tools/refresh-fingerprint.sh`; there are no GLM-specific fingerprint env vars): a per-profile UA
pin from the pool, the full 10-beta `anthropic-beta`, `x-app` + the entire `x-stainless-*` set,
`accept`, `anthropic-dangerous-direct-browser-access`, identity as the first system block without
double injection, and a per-profile billing block `x-anthropic-billing-header: cc_version=<base>.dNN;
cc_entrypoint=…; cch=<hex>` (cch and `.dNN` are deterministic on the roster profile id, like the
per-persona scheme of the Claude pool). Client identity headers are not forwarded to the
upstream — the persona is synthesized in full (someone else's `x-stainless-*` under our claude-cli
UA is a contradiction on which SDK detection trips). The quota endpoint carries no identity set:
it is a monitor surface, not generation. "Bare SDK" traffic is a direct path to a subscription
ban. Fallback values — a reviewed capture of Claude Code 2.1.195; if the §6.7 live gate shows
that Z.ai does not accept some beta, the set is trimmed via `CLAUDE_API_BETA` without a rebuild.

`unknown` Whether the Anthropic endpoint accepts `x-api-key` (only Bearer is documented).
`decision` We implement Bearer; no alternative is needed until live proof of the contrary.

`unknown` The exact form of `usage` on the Anthropic route (cache field names, the thinking-token
counter). Without authoritative usage, billing fails closed — settlement against the
conservative hold.

### 4.1 Implemented backend gateway

`decision` Exact reviewed GLM aliases (`glm-5.2`, `glm-5-turbo`, `glm-4.7`, `glm-5.2[1m]`) are
dispatched inside the Anthropic `POST /v1/messages` after common authorization and bounded body
reading, but before Claude-specific identity, pricing, and pool mutation — following the pattern
of KIMI §4.1. An alias never goes to the Claude upstream: a disabled plane, a corrupted initial
roster, and a cold roster produce the GLM path's fail-closed response, not a fallback.

### 4.2 Error classes

`official` (`docs.z.ai/api-reference/api-code`, revision 2026-07-30): a two-layer scheme —
HTTP status + business code in the body `{"error": {"code": "1308", "message": "…"}}`:

| Code | HTTP | Meaning | Our reaction |
|---|---|---|---|
| 1000–1005 | 401 | auth failure | auth quarantine (no refresh — the key is static), rotate |
| 1113 | 429 | insufficient balance (out-of-plan call) | account anomaly → suspect, not a quota wall |
| 1210–1215 | 400 | request validation | client semantic error, no rotate/blame |
| 1220 | 403 | access denied | capability/plan → fail closed scope |
| 1301 | 400 | content filter | client semantic error |
| 1302 | 429 | rate limit | bounded transport rotation |
| 1305 | 429 | overload | bounded transport rotation |
| 1308 | 429 | **5-hour quota exhausted**, "reset at {next_flush_time}" | quota wall → cooling until parsed reset, no transport budget |
| 1309 | 429 | plan expired | account dead (out of rotation until key replacement) |
| 1310 | 429 | **weekly/monthly quota exhausted** | quota wall → cooling until weekly reset |
| 1311 | 429 | model not in plan | model-scope ineligible, not account |
| 1313 | 429 | fair-use violation | account suspect (risk control), out of rotation |
| 1315 | 429 | key limited to enterprise scenarios | wrong key kind → suspect (out of rotation, operator review) |
| 1316–1321 | 429 | extra usage / monthly spend limit (Team mechanics) | account anomaly → suspect |

`official` On an abnormal SSE break, error codes are not returned — the reason arrives in the
chunk's `finish_reason`. `decision` Mid-stream classification must read `finish_reason`
(`sensitive`, `model_context_window_exceeded`, `network_error`).

`oss-hypothesis` The quota endpoint answers HTTP 200 even with `code: 401` in the body.
`decision` The quota handler must check the business code, not only the HTTP status.

## 5. Money / quota

### 5.1 Official rate card (replacement cost)

`official` `docs.z.ai/guides/overview/pricing`, reviewed 2026-08-03, USD, per 1M tokens:

| Model | Input | Cached input | Cached storage | Output |
|---|---|---|---|---|
| `glm-5.2` (= glm-5.1) | $1.40 | $0.26 | limited-time free | $4.40 |
| `glm-5-turbo` | $1.20 | $0.24 | limited-time free | $4.00 |
| `glm-5` | $1.00 | $0.20 | limited-time free | $3.20 |
| `glm-4.7` (= 4.6, 4.5) | $0.60 | $0.11 | limited-time free | $2.20 |
| `glm-4.5-air` | $0.20 | $0.03 | limited-time free | $1.10 |

`official` Cache storage — "Limited-time Free". `decision` As with KIMI: the paid cache-write leg
is **absent** as a documented fact, rather than silently counted as zero; if the "limited-time"
marking is removed, the schedule is updated with a new epoch.

`official` The Web Search tool on Open Platform — $0.01/use; the Coding-Plan MCP (Web Search /
Web Reader / Zread / Vision) is charged in credits (×1.2 per call). `decision` The tool/search
capability on our route is recorded as **unavailable** until a bounded per-request ceiling is
proven (`SKILL.md`); no budget is spent.

`official` Reasoning: `reasoning_content` is returned as a separate field; the thinking-token
counter in `usage` is not documented. `decision` Reasoning is a **subset of output**
(conservatively: completion_tokens includes reasoning), not billed as a separate leg; the
invariant reasoning ≤ output is checked wherever the provider returns a breakdown.

### 5.2 Native quota — credits and the quota endpoint

`official` The credits formula (devpack/overview):

```text
credits = (input_tokens × in_mult + cached_input_tokens × cache_mult
           + output_tokens × out_mult) / 10 000
```

| Product | in_mult | cache_mult | out_mult |
|---|---|---|---|
| GLM-5.2 | 6.9 | 1.7 | 24 |
| GLM-5-Turbo | 5.7 | 1.5 | 21 |
| GLM-4.7 | 4.6 | 1.2 | 16 |
| GLM-4.6V (Vision MCP) | 1.2 | 0.3 | 2.7 |
| MCP tools (per call) | — | — | 1.2 |

`official` Off-peak: charging ×0.5 outside peak (Mon–Fri 14:00–18:00 SGT = UTC+8).

`oss-hypothesis` Quota endpoint `GET {base}/api/monitor/usage/quota/limit`
(onWatch `zai_types.go`, pinned SHA — §8): wrapper `{code, msg, success, data}`,
`data.limits[]` with `type: "TIME_LIMIT"|"TOKENS_LIMIT"`, fields
`unit, number, usage, currentValue, remaining, percentage, nextResetTime` (epoch ms),
`usageDetails[].modelCode` — per-model breakdown. Invalid key: HTTP 200 + `code: 401`.

`unknown` The unit semantics of `currentValue/remaining/number` (credits? tokens? what do
`TIME_LIMIT` vs `TOKENS_LIMIT` mean?) are not proven. Per `SKILL.md`, the values are preserved
as **raw quota evidence** until live proof and are not divided by a token price.

### 5.3 Choosing the ledger model

`decision` GLM is a **GPT-like dual-ledger provider with published native limits**:

1. **API-nanoUSD ledger** — exact, per-turn, from the rate card §5.1 by **served** model.
2. **Native credits ledger** — per-turn, computed from the official formula §5.2 (including
   off-peak ×0.5 on the UTC+8 schedule). This is not a "derived" value: the formula and the
   multipliers are published by the provider; the ledger is independent of API dollars and is
   never reconstructed from them.
3. **Window observations** — the quota endpoint (§5.2) gives the provider-side window state
   (`remaining`, `nextResetTime`); it is preserved as immutable raw evidence with its own
   resolution.

There is no need to estimate the native window capacity — it is published (2 000/12 000/28 000
credits per 5h; 10 000/60 000/140 000 per week by plan). Only the official API replacement cost
fitting into the window at the observed load is subject to estimation: `capacity_nanoUSD =
native_limit × ΣΔapi_nano / ΣΔnative_credits` over complete intervals (form §10.5 of onboarding,
checked integer math, round-half-up).

`decision` A divergence of our computed credits ledger from the provider-side quota endpoint is an
expected evidence class: off-peak rounding, MCP calls, legacy plans, and **foreign consumption**
(all supported tools share one account quota) are visible as unattributed movement and are not
attributed to our gateway. Legacy prompts-form observations are incompatible with the credits
model and fail closed (profile quarantine, operator review) rather than being silently
interpreted.

`decision` Cohorts (§10.6) are merged only by exact declared plan + exact window duration, with
corroboration by the observed native limit. An unknown/legacy plan blocks aggregation.

### 5.4 Runtime ordering

`decision` Mirrors KIMI §5.4: the first free quota anchor after key validation, then a periodic
poll (`CLAUDE_API_GLM_QUOTA_POLL_SECS`); roster discovery is an independent 15-second tick. The
turn-before-quota ordering is mandatory: the poll is not executed with a pending head in the
bounded turn FIFO; after the HTTP snapshot the writer drains the FIFO again, reads the durable
cumulative ledgers, and completes the immutable observation/CAS before publishing quota steering.

## 6. What remains unproven

Each `unknown` fails closed and is cleared only by a controlled live run on our own subscription:

1. The exact form of `usage` on the Anthropic route (cache fields, thinking counter).
2. Real SSE incrementality (a buffered frame ≠ stream).
3. The unit semantics of the quota endpoint (`currentValue/remaining/number`, TIME_LIMIT vs
   TOKENS_LIMIT).
4. Distinguishing a legacy prompts plan from a credits plan via the quota endpoint on a live
   account.
5. Whether disabling thinking changes the served model/tariff.
6. Quota-wall behavior on the Anthropic route: the exact business code and body (1308/1310).
7. Whether the Anthropic endpoint accepts `x-api-key`; the full set of mandatory identity headers
   that pass risk control without throttling.
8. The presence and accuracy of per-model `usageDetails` for unattributed attribution.
9. Current USD/CNY plan prices (does not block: the price does not participate in calculations).

## 7. Auth Bot: acquisition flow (decision)

`decision` A separate `HandoffKind::Glm`, steps `glm_proxy → glm_ready → glm_wait`, button
`glm:ready`. The difference from KIMI — there is no device flow; instead, the key is entered as
text:

1. `glm_proxy`: proxy as text (reversible parsing, canonicalization via
   `glm_credential::normalize_proxy_url`), choice of the int (`api.z.ai`) or CN
   (`open.bigmodel.cn`) platform by button.
2. The seller receives a newcomer guide: do not open the account without a proxy, do not change
   the profile/IP, activate the exact plan (Lite/Pro/Max per the offer product), create a key in
   the console under Plan Overview.
3. `glm_ready`: the seller confirms account readiness; the bot asks them to send the API key.
4. `glm_wait`: the bot validates the key: a free quota probe (§2.1) → one minimal paid generation
   on the Anthropic route (`glm-4.7`, `max_tokens=1`, aggregate cap $0.0001 per the AGENTS.md
   admission micro-smoke) → seal envelope → atomic roster publish → payout completion.
5. Cancel/retry/expiry/wrong-plan leave neither a credential file nor a roster row and do not
   complete the payout. An invalid key (HTTP 200 + `code:401`), a Team/legacy quota form, or a
   plan↔limit mismatch — return to `glm_ready` with a safe hint.

## 8. Delivery status

| Stage | Artifact | Status |
|---|---|---|
| research / capability manifest | this file | done |
| official rate card + credit multipliers | `crates/metering/src/glm.rs` | done, 24 tests |
| calibration authority (schema 0029) | `crates/registry/migrations_pg/0029_glm_window_calibration.sql` | done, expand-only, real-PG matrix green |
| observation types | `crates/registry/src/glm_calibration.rs` | done, 20 tests |
| credential | `crates/glm-credential` | done, 18 tests |
| calibration estimator | `crates/forward/src/glm_calibration.rs` | done, 27 tests |
| Auth Bot: validation protocol + roster | `crates/authbot/src/{glm_key,glm_roster}.rs` | done, 26 tests |
| Auth Bot: seller wizard | `crates/authbot/src/bot.rs` (+`db.rs` `hregion`/recovery, `main.rs`) | done, 21 tests (wizard, menu, region, restart recovery) |
| runtime primitives: config / transport / roster / client / selection / pool / queue | `crates/forward/src/glm/` | done, 71 tests |
| gateway (+ dispatcher `proxy.rs`, `AppState.glm`, billing writer, test-loopback credential feature) | `crates/forward/src/glm/gateway.rs` | done, 35 mock tests + real-PG gate in `billing.rs` |
| server: env/config + composition | `crates/server/src/{config,main,poller}.rs` | done (env/config, composition, maintenance loop, shutdown flush) |
| observability, alerts, admin projection | `crates/{forward,server}`, `observability/**`, `docs/ops/MONITORING.md` | done: operational status, admin-only `GET /glm-subs` (+`window_totals` for the fleet card), fixed-cardinality aggregate metrics, `glm-provider` alerts with runbook and consistency pins |
| admin UI consumer | `apps/admin` | done: same-origin consumer of `/glm-subs` — GLM capacity board on `/subscriptions` (fleet card, per-account windows, exact API-$, native microcredits) |
| safe live-runner | `tools/glm_calibration/`, `docs/ops/GLM_CALIBRATION.md` | done, awaiting the first subscription |
| production activation boundary | `systemd/claude-api{,@,-anthropic@}.service` | public Anthropic/combined units pin `CLAUDE_API_GLM_ENABLED=0`; activation blocked on owned live evidence plus a compliant private boundary or written permission |
| live matrix on our subscription | — | **subscription needed (blocked on a human)** |

Queue and SHA tracking — `research/GLM_PLANE_PROGRESS.md`.

## 9. Sources

All links reviewed 2026-08-03.

- `https://docs.z.ai/devpack/overview` — plans, credits, formula, off-peak, reset
- `https://docs.z.ai/devpack/faq` — endpoints, model trio, exhaustion behavior
- `https://docs.z.ai/devpack/quick-start` — obtaining the key, endpoint guide
- `https://docs.z.ai/devpack/usage-policy` — rate limits, sharing prohibition, sanctions
- `https://docs.z.ai/devpack/notice/usage-revision` — prompts→credits transition 2026-07-30,
  legacy, Team
- `https://docs.z.ai/guides/overview/pricing` — official rate card
- `https://docs.z.ai/guides/llm/{glm-5.2,glm-5-turbo,glm-4.7}` — contexts, max output
- `https://docs.z.ai/api-reference/llm/chat-completion` — OpenAI wire, usage, thinking
- `https://docs.z.ai/api-reference/api-code` — error codes
- `https://docs.z.ai/legal-agreement/subscription-terms` — resale/proxy/multi-user prohibition
- `github.com/onllm-dev/onwatch` @ main (2026-08-03), `internal/api/zai_client.go`,
  `internal/api/zai_types.go` — quota endpoint (read-only research; MIT-like license verified
  while reading; no temporary clone was created, read via the web)
- `github.com/gsd-build/gsd-2#3874` — Bearer on `/api/anthropic` (wire dump)
- `github.com/QuantumNous/new-api#2051` — reseller coding-plan channel scheme
- `github.com/earendil-works/pi#4187` — risk-control "SDK-based access"
