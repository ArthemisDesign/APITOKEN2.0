# KIMI (Moonshot AI) — provider capability manifest

Integration status: **default-off backend preview runtime, mock-verified and not yet live-verified**.
Source review date — **2026-08-03**.

This document was created per `docs/engine/PROVIDER_ONBOARDING.md` §3.3 and is the capability manifest
of the KIMI plane. It records what is proven, by what exactly it is proven, and what remains `unknown`.
Every claim below is labeled per the evidence hierarchy from §3.1: `official`, `live`,
`oss-hypothesis`, `decision`, `unknown`, `not-applicable`.

## 0. Scope and intentional limitations

The KIMI plane was built **backend-only**, and that scope was widened by the product owner on
2026-08-08: the provider is now published to the unified **router** catalog under the `kimi/*`
namespace. It remains unpublished on the marketing site. Commerce/OpenKeys pricing surfaces and
client documentation are in progress and are tracked separately.

- `decision` 2026-08-08 — publish through the router, not through the plane's own `/v1/models`.
  `GET /v1/models` on the Anthropic plane is a byte-for-byte proxy of `api.anthropic.com`, and
  KIMI rides that same plane because it speaks Anthropic Messages. Appending our aliases there
  would break transparency for every client, including those who never asked for KIMI. Discovery
  therefore goes through a new internal producer, `GET /internal/router/catalog/kimi`, and the
  router treats KIMI as a fourth **catalog plane** over the existing Anthropic **lane**. See
  "Catalog planes vs protocol lanes" in `docs/engine/UNIFIED_ROUTER.md`.
- `decision` 2026-08-08 — advertise only the five subscription aliases. The official Open
  Platform ids are tariff keys the gateway refuses on the wire, so publishing one would put a
  model in the catalog that admission then rejects. `kimi-k2.6` is not advertised at all: no
  alias reaches it.
- `decision` The site keeps no KIMI row until the owner says otherwise.
- `decision` This file is an internal engineering instruction (required by `AGENTS.md`,
  "Documentation is a living contract"), not a public storefront.

A key under **strict** policy still cannot use KIMI: the gateway refuses it outright
(`kimi_strict_pricing_unavailable`) because the pricing release catalog has no `kimi` provider,
and the router consequently quotes no `kimi/*` rate for such a key. This is the same position
Gemini is in, and it is a known gap rather than a router defect. It does not block publication:
since generation 55 was ratified as final, new models ship through the engine's
`is_model_unpriced` → exact-legacy-tariff fallthrough plus a hot tariff seed, which is exactly
the path KIMI already reserves on.

Until the live matrix is executed, the GA criterion of §1 `PROVIDER_ONBOARDING.md` is **not
claimed**. The terminal state of this work is a verified **preview**: runtime and calibration are
proven on mock gates; live gates are executed separately on our own subscription.

## 1. Product / plan

`official` Kimi separates three independent access systems; keys and base URLs are not
interchangeable between them (Kimi Code docs, error-reference).

| Plane | Purpose | Base URL | Billing |
|---|---|---|---|
| Kimi Open Platform | pay-per-token developer API | `https://api.moonshot.ai/v1` (int.), `https://api.moonshot.cn/v1` (CN) | per token |
| Kimi Code (subscription) | subscription coding plan | `https://api.kimi.com/coding/v1` (OpenAI), `https://api.kimi.com/coding/` (Anthropic) | from subscription quota |
| Kimi web/app chat | consumer chat | — | subscription, **provides no API** |

`decision` Our provider is **Kimi Code only**. Open Platform is used exclusively as the
authority for official pricing (replacement cost), not as a source of capacity.

### 1.1 Pricing plans

`unknown` **The exact set and prices of plans are not pinned down.** Sources contradict each other:
the Kimi help center lists a CNY ladder (Adagio ¥0, Andante ¥49, Moderato ¥99, Allegretto ¥199,
Allegro ¥699), the international pages list a USD ladder (Adagio free, Moderato $19, Allegretto $39,
Allegro $99, Vivace $199), and as of 2026-07-20 a split between general membership and a separate
Coding Plan with different tier names is reported. None of these numbers is confirmed by a
provider-owned page at review time.

`decision` The subscription price **does not participate** in calculations: calibration answers the
question "how much official API replacement cost fits into the window", not "how much the
subscription cost" (`PROVIDER_ONBOARDING.md` §10). Therefore the unknown price **does not block**
the integration.

`official` What is pinned firmly — **the plan name is machine-readable**: the `/me`
endpoint returns `user_level` (int) and `user_level_name` (string, e.g. `"Vivace"`). This is the
authoritative paid plan identity for calibration cohorts. The marketing price is not needed for this.

`official` Capability gating by tiers (Kimi Code docs, models):

| Capability | Required tier |
|---|---|
| `kimi-for-coding` (256K) | any member |
| `k3`, `k3-256k` (256K) | Moderato and above |
| `k3` full 1M context | Allegretto and above |
| `kimi-for-coding-highspeed` | Allegretto and above |

`official` Requesting a capability above the plan returns `401` (models docs) or `403`
(error-reference). The source discrepancy is `unknown`; the handler must classify both as
"capability not permitted by plan", not as auth death.

`decision` **2026-08-07 — the ladder above is encoded in `KIMI_REVIEWED_PLANS`.** It was empty
until this date, which cost more than it protected: an empty table cannot tell the free tier from
the most expensive one, so every subscription collapsed to `kimi-for-coding` at 256K. Our own
`supports()` refused `k3`/1M/highspeed before any request left the process, the pool reported "no
profile", and the transparent envelope returned a `429` indistinguishable from an upstream rate
limit — with the plan named nowhere in the chain. Verified live on 2026-08-07: the paid matrix on
the connected Vivace subscription could reach only the base model, and the refusal was ours.

The emptiness was justified by contradictory sources, but the contradiction is in the **price**
ladder (CNY versus USD names, the mid-2026 coding-plan split), not in the capability mapping above,
which is `official`, nor in the plan identity, which `/me` publishes machine-readably as
`user_level_name`. Entries therefore carry the same meaning as `GLM_REVIEWED_PLANS`: confirmed
against official documentation on the stated date. Adagio and Andante get base capabilities only,
Moderato adds `k3`/`k3-256k`, and Allegretto, Allegro and Vivace additionally add the 1M window and
highspeed. Lookup ignores case and padding so a spelling difference from `/me` cannot silently cost
a subscription its tier; a plan outside the ladder still fails closed to base and must remain
operationally visible instead of degrading quietly.

`official` **Community Guidelines restrict the subscription to "personal interactive use only".**

> `decision` This restriction is a real compliance risk for reselling capacity. It is precisely
> this that additionally justifies the backend-only mode without publication. Any expansion to
> resale requires a separate legal review and is not performed in this work.

## 2. Credential

`official` + `oss-hypothesis` Official CLI `github.com/MoonshotAI/kimi-code`, MIT,
pinned SHA `75395f6abb17f83f30d16b51f4e060a639f43622` (2026-08-03), `packages/oauth/src`.

| Field | Value | Label |
|---|---|---|
| Grant | OAuth 2.0 Device Authorization Grant (RFC 8628) | `oss-hypothesis` |
| OAuth host | `https://auth.kimi.com` | `oss-hypothesis` |
| Device authorization | `POST /api/oauth/device_authorization`, form, `client_id` | `oss-hypothesis` |
| Token | `POST /api/oauth/token`, form | `oss-hypothesis` |
| Device grant type | `urn:ietf:params:oauth:grant-type:device_code` | `oss-hypothesis` |
| Refresh grant type | `refresh_token` | `oss-hypothesis` |
| Official client id | `17e5f671-d194-4dfb-9706-5516cb48c098` | `oss-hypothesis` |
| PKCE | `not-applicable` (device flow) | `decision` |
| Scopes | response contains `scope`, concrete values not documented | `unknown` |
| Refresh rotation | **rotating family**: `refresh_token` is mandatory in the refresh response | `oss-hypothesis` |
| Alt-credential | static API key from Kimi Code Console | `official` |

`decision` The rotating refresh-token family means a mandatory per-profile single-flight
re-seal per `PROVIDER_ONBOARDING.md` §6: the winner atomically re-seals the envelope before
releasing the lock; the loser re-reads the envelope once. Uncontrolled reuse of an old refresh
token kills the subscription.

`decision` The device flow maps perfectly onto the Auth Bot: the seller receives a `user_code` and
`verification_uri_complete`, confirms in the browser, and the bot polls `/api/oauth/token`. The
seller never hands over a password, 2FA, or the token itself.

### 2.1 Identity — `GET {base}/me`

`oss-hypothesis` Header `Authorization: Bearer <access_token>`. Payload:

| Field | Role |
|---|---|
| `user_id` | **stable provider subject** — quota and dedup authority |
| `user_level`, `user_level_name` | **authoritative paid plan identity** for cohorts |
| `status` (`USER_STATUS_NORMAL`) | account state |
| `region` (`REGION_CN`) | inference geography |
| `email`, `phone`, `nickname`, `avatar` | **PII — sealed, never published outward** |

`decision` Only an opaque id derived from `user_id`, plus `user_level_name`, is exposed outward
(admin projection, metrics, logs). `email`/`phone`/`nickname` never leave the envelope — §12 forbids
full email and external account id in the projection.

## 3. Model admission

`official` Subscription ids (Kimi Code docs, models) and their mapping to the official
Open Platform rate card (`platform.kimi.ai/docs/pricing/*`).

| Subscription model | Official model (rate card) | Context | Tier | Non-stream | Incremental stream | Usage | Quota | Decision |
|---|---|---|---|---|---|---|---|---|
| `kimi-for-coding` | `kimi-k2.7-code` | 262 144 | all | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |
| `kimi-for-coding-highspeed` | `kimi-k2.7-code-highspeed` | 262 144 | Allegretto+ | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |
| `k3-256k` | `kimi-k3` | 262 144 | Moderato+ | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |
| `k3` | `kimi-k3` | up to 1 048 576 | Moderato+ (1M — Allegretto+) | `unknown` | `unknown` | `unknown` | `official` | preview, behind switch |

`official` `k3[1m]` is **not a separate model**, but a Claude-Code-specific notation
that enables the 1M window. Ordinary API calls use `k3`. Our canonical id is `k3`
with an explicit context mode; the bracket form is accepted as an alias.

`decision` **2026-08-08 — an alias and its wire id are separate fields.**
`KimiSubscriptionModel` now carries `wire_model` beside `alias`. For every id the provider
publishes the two are equal; only `k3[1m]` differs and goes out as plain `k3`. This was a live
defect, not a cleanup: the gateway forwarded the request body's `model` verbatim, so the bracket
form reached an endpoint that has no such id. Every `k3[1m]` request failed upstream and the
exhausted rotation surfaced it as a capacity `429` naming neither the model nor the cause, while
`k3` — same tariff, same window, same capability branch — answered 200 on the same profile. The
rewrite happens once before anything reads the body, so selection, pricing, the immutable turn
event and attribution all continue to use the requested alias.

`official` **Reasoning controls differ by family:**

- `k3`: always reasons, `reasoning_effort` ∈ {`low`, `high`, `max`}, default `high`.
  Alias normalization: `null`/`undefined`→`high`, `ultra`/`max`/`xhigh`→`max`,
  `high`/`medium`→`high`, `low`/`minimum`/`light`→`low`, `none`→thinking off,
  anything else → HTTP 400.
- `kimi-for-coding` / `-highspeed`: `Thinking: ON`.

`official` **Critical for money: disabling thinking routes the request to K2.6.**
"Disabling thinking routes both K3 and K2.7 Code to K2.6." That is, **served model ≠ requested
model**, and K2.6 has a different rate card (cache hit $0.16 vs $0.30 for K3).

> `decision` Therefore the immutable turn event must store the requested and served models
> **separately** (`PROVIDER_ONBOARDING.md` §10.2), and metering runs against the **served** model
> taken from the provider's response, not the requested one. If the provider did not return a
> served model and thinking is off — the model is treated as unknown and billing fails closed at reserve.

`official` `kimi-for-coding-highspeed` silently degrades to `kimi-for-coding`
on a typo, without an error. `decision` → the served model is always taken from the response,
never from the request.

`official` `k3` at 1M "consumes roughly twice as much quota" as `k3-256k`;
highspeed — "6× speed, 3× quota consumption". This confirms that the native quota is a
**weighted credit**, not a request counter.

`unknown` The exact quota weights (2×, 3×) are not a public normative contract with a unit of
measurement. Their semantics can only be proven by a live run.

## 4. Wire

| Operation | URL | Headers | Body | Framing | Usage | Errors |
|---|---|---|---|---|---|---|
| Generation (Anthropic) | `POST https://api.kimi.com/coding/v1/messages` | `Authorization: Bearer` (CLI) or `x-api-key` (Claude Code) | Anthropic Messages | SSE | `unknown` | see §4.2 |
| Generation (OpenAI) | `POST https://api.kimi.com/coding/v1/chat/completions` | `Authorization: Bearer` | OpenAI Chat | SSE | `unknown` | see §4.2 |
| Catalogue | `GET /coding/v1/models` | `Authorization: Bearer` | — | JSON | — | **ungated** |
| Identity | `GET /coding/v1/me` | `Authorization: Bearer` | — | JSON | — | 401 |
| Quota | `GET /coding/v1/usages` | `Authorization: Bearer` | — | JSON | — | 401/404 |

`decision` **The Anthropic-compatible transport is the decisive architectural advantage.** The
engine's native protocol is already Anthropic (`CLAUDE.md`, "Transparency" invariant). The KIMI
plane therefore needs no protocol translation on the scale of `crates/forward/src/gemini/`
(schema/stream/skin — about 10,000 lines); it reuses the existing Anthropic path and adds only
credential, transport, pool, metering, and calibration.

`official` `GET /v1/models` **does not verify authorization** — it answers 200 to an invalid key,
while a subsequent generation returns 403. `decision` → the plane's readiness probe must hit
`/messages` with a minimal request or `/me`, but **never** `/models`. A false-positive health
check via `/models` is expressly forbidden.

`unknown` The exact auth header of the Anthropic route (`Authorization: Bearer` vs `x-api-key`)
is not confirmed by a normative page. The official CLI uses Bearer for `/me`, `/usages`, and
chat; the Claude Code documentation sets `ANTHROPIC_API_KEY`, which yields `x-api-key`.

`oss-hypothesis` The official CLI identifies itself with the string `kimi-code-cli/<version>`
(changelog: the User-Agent is sent so registries can determine the client version). The engine pins
`kimi_credential::KIMI_CODE_CLI_USER_AGENT` to the version of `apps/kimi-code/package.json` from the
pinned research SHA — a bare HTTP client without a UA risks looking like a foreign bot to the
subscription endpoint.
`decision` → we implement Bearer as the source-verified variant, `x-api-key` as a
configurable alternative; the choice is pinned by the very first live run.

`unknown` The form of terminal usage on the Anthropic route (the `usage` fields, presence of
`cache_read_input_tokens` / `cache_creation_input_tokens`) is not confirmed. Without authoritative
usage, billing fails closed — settlement against the conservative hold.

### 4.1 Implemented backend gateway

`decision` Exact reviewed Kimi Code aliases are dispatched inside the Anthropic
`POST /v1/messages` after common authorization and bounded body reading, but before
Claude-specific identity, pricing, and pool mutation. An alias never goes to the Claude upstream:
a disabled plane, a corrupted initial roster, and a cold roster produce the KIMI path's
fail-closed response, not a fallback.

`decision` The implementation in `crates/forward/src/kimi/gateway.rs` proves the following local
invariants on mocks, but does not clear the provider-owned `unknown`s of §6:

- non-stream responses and SSE bytes pass through with no protocol translation; the usage extractor
  can assemble split SSE frames, but settlement accepts only the terminal event as authoritative;
- retry/rotation are permitted only before the first public byte; after it, the upstream is drained
  even on downstream disconnect, and shutdown waits for the stream finalizer;
- a metered turn passes customer reserve → durable delivering marker → terminal settlement;
  the actual charge is taken by **served model**, and missing terminal usage preserves the full hold;
- immutable turn evidence is delivered through a bounded FIFO; `/usages` is not executed with a pending
  head, and after the HTTP snapshot the writer drains the FIFO again, reads the durable cumulative
  spend, and completes the immutable observation/CAS before publishing quota steering;
- the poll takes only an idle profile snapshot and introduces no customer semaphore: a generation
  that starts during the HTTP changes the monotonic epoch and invalidates the snapshot entirely.
  After the final epoch check a new turn may proceed in parallel, but its enqueue is held by the
  FIFO barrier until the earlier quota snapshot is written;
- OAuth refresh holds a per-profile single-flight, requires a new rotating refresh family, and
  atomically re-seals the envelope before releasing the lock; the blue-green loser re-reads the
  shared disk authority;
- readiness checks only authenticated `/me`; the first 401 forces one forced refresh/retry;
- the server looks for a new atomic roster publication every 15 seconds; an unchanged profile keeps
  the same runtime `Arc` with its health/client/in-flight state, while a new or changed credential
  passes `/me` before the entire generation is published;
- malformed/decrypt/client/probe failure and a missing `profiles.json` preserve last-good
  capacity. Intentional fleet removal is expressed only by a valid empty roster; a removed profile
  is immediately closed to new requests, but an already issued in-flight lease lives until its
  natural drop;
- before the atomic swap the gateway takes the affected refresh locks and re-reads the roster: a
  snapshot made stale by a parallel rotating refresh/re-seal cannot become the new in-memory authority;
- bearer redirect is forbidden; unsupported reasoning fails closed. Client-side tool
  declarations, `tool_use`/`tool_result` blocks and inline media are accepted: they are text in
  the request body, and the customer hold reserves one input-token price per body byte, so
  their cost is bounded before dispatch. Only provider-executed work — `mcp_servers` and
  server-side search/computer/code-execution tools — stays refused, because it may bill a unit
  that is invisible in the body and not proportional to it (unknown 8);
- synthetic errors pass through the common Anthropic-compatible sanitizer and do not disclose the
  internal backend name, roster, subscription, or provider body to the client;
- an unverified plan receives only the base `kimi-for-coding`; the reviewed tier allowlist remains
  the authority for extended aliases.

### 4.2 Error classes

`official` (Kimi Code error-reference):

| Status | Meaning | Our reaction |
|---|---|---|
| 401 | auth failed, or capability above plan | refresh + retry on the same profile; repeat → auth quarantine |
| 402 | "unable to verify your membership benefits", usually temporary | transport class, bounded rotation |
| 403 | identity is valid, but: tier does not grant the capability / account closed / **quota exhausted** | quota wall → cooling until reset, no transport budget |
| 429/5xx | inference overload — "retry directly" | bounded transport rotation |

`official` The provider itself separates "engine overload (retry makes sense)" and "account
quota (retry is useless)". `decision` → this is exactly the axis separation of §8.4: quota bucket
separate from transport health.

`decision` **2026-08-08 — an environment-derived reason may never take the last capacity out of
service.** Selection now runs a second, relaxed pass when the strict pass finds nothing: a profile
held only by `AuthQuarantined` or `TransportWedged` becomes selectable again. Both are our own
reading of the environment, not a provider statement about capacity — an auth refusal on this plane
arrives *after* a successful token refresh, and the provider returns 401 for a proxy-side block and,
per its own error reference, for "capability above your plan" as well. `QuotaWall` and
`CapabilityNotInPlan` are provider verdicts and stay walls; relaxing them would burn a request that
cannot succeed. `ModelCooling` also stays, because it is scoped to one model while the profile's
other models keep serving.

Without this, one refusal wave walking the roster zeroes the plane for the full
`AUTH_QUARANTINE_SECS`. That is not hypothetical: Gemini's pool went to zero nine times over two
days in August 2026 for exactly this reason, Codex carries `admission_ignoring_soft_cooling` for
it, and Claude never had the defect. KIMI was the last plane without the hatch.

`unknown` 403 combines quota exhaustion and missing plan capability. They can be distinguished
only from the error body; until live confirmation the handler fails closed by classifying an
indistinguishable 403 as a quota wall (conservatively — the profile is taken out of rotation
until reset rather than marked dead).

## 5. Money / quota

### 5.1 Official rate card (replacement cost)

`official` `platform.kimi.ai/docs/pricing/*`, reviewed 2026-08-03, USD, before taxes,
"1M = 1 000 000". There are no pricing steps by context length — the rate is flat across the
entire window.

| Official model | Cache hit / 1M | Cache miss / 1M | Output / 1M | Context |
|---|---|---|---|---|
| `kimi-k3` | $0.30 | $3.00 | $15.00 | 1 048 576 |
| `kimi-k2.7-code` | $0.19 | $0.95 | $4.00 | 262 144 |
| `kimi-k2.7-code-highspeed` | $0.38 | $1.90 | $8.00 | 262 144 |
| `kimi-k2.6` | $0.16 | $0.95 | $4.00 | 262 144 |

`official` No separate cache **write**/storage price is published — only hit and miss. Caching is
described as automatic. `decision` → the cache-write leg is absent, rather than silently counted
as zero; the zero here is a documented fact of a missing paid leg.

`official` Reasoning tokens are billed at the output rate (K3: "reasoning output consumes
tokens billed at the output rate"). `decision` → reasoning is a **subset of output**, not billed
as a separate leg; there is no double counting.

`official` Web search on the platform is "currently being updated", its use is not recommended,
and the documentation is outdated. `decision` → **the capability is recorded as unavailable, and
no budget is spent** (`SKILL.md`: a paid tool/search is dispatched only when a bounded per-request
ceiling is proven). The current pricing pages contain no separate per-call price.

`official` Deprecated/removed: the `kimi-k2-*` series was removed 2026-05-25, `kimi-latest` —
2026-01-28, `kimi-thinking-preview` — 2025-11-11, `moonshot-v1-*` and `kimi-k2.5` — sunset
August 31. `decision` → they are not entered into the rate card: the subscription does not serve them.

### 5.2 Native quota — `GET /coding/v1/usages`

`oss-hypothesis` Schema (official CLI, `packages/oauth/src/managed-usage.ts`):

```json
{
  "usage":  { "used": "40", "limit": "1000", "resetTime": "2026-08-03T05:20:51Z" },
  "limits": [
    { "name": "...",
      "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
      "detail": { "used": "1", "limit": "100", "resetTime": "..." } }
  ],
  "boosterWallet": {
    "balance": { "type": "BOOSTER", "amount": <fp>, "amountLeft": <fp> },
    "monthlyChargeLimitEnabled": false,
    "monthlyChargeLimit": { "priceInCents": <int>, "currency": "USD" },
    "monthlyUsed": { "priceInCents": <int>, "currency": "USD" }
  }
}
```

Material properties:

- `used`/`limit` are **integer decimal strings**, not percentages. `decision` → we parse them into
  integers, not through float; the fraction is computed as `used * FRACTION_SCALE / limit`.
- **Window resolution = `FRACTION_SCALE / limit`.** This is fundamentally better than Claude's
  whole-percent: at `limit=1000` the resolution is 0.1 %. `decision` → the resolution is computed
  from the actual `limit` of each window and stored together with the observation (§10.3), rather
  than set by a constant.
- `usage` is a **weekly** window; the backend does not send a `window`, the CLI synthesizes
  `1 week`. `decision` → we **do not** silently synthesize the window: the weekly semantics are
  confirmed `official` ("refreshes automatically every 7 days"), and this is recorded as an
  explicit mapping with the source cited.
- `limits[]` — windows with explicit duration; the 5-hour one arrives as `duration: 300`,
  `TIME_UNIT_MINUTE`. `decision` → the duration is normalized to seconds and stored exactly;
  the 5h and 7d windows live **independently** (§10.3).
- `resetTime` — RFC3339, direct reset evidence for the interval state machine.
  `decision` → a missing, impossible, or non-strict timestamp rejects the entire snapshot;
  normalization to Unix seconds happens before the durable write, with no local `now + duration`.
- `boosterWallet` — Extra Usage, real money in fixed-point (divisor 1 000 000 → cents),
  with currency USD/CNY. `decision` → this is a **third, separate** ledger: it is mixed with
  neither the native quota nor API dollars.

`unknown` **The unit of `used` is not proven.** Indirectly it is weighted (K3@1M ≈ 2× relative to
`k3-256k`, highspeed = 3×), but there is no normative definition: it may be a credit, a token
equivalent, or a weighted request. Per `SKILL.md` ("provider buckets publish amounts with unclear
semantics"), the value is preserved as **raw quota evidence** until live proof and is **not
divided** by a token price to invent capacity.

`official` The membership monthly ceiling is shared: "PPT, Agent Cluster, Kimi Code etc. share a
common monthly limit"; when it is exhausted, Kimi Code is frozen even with weekly quota remaining.
`decision` → this is a third, **external** window; its exhaustion shows up as a 403 with a non-empty
weekly quota. The handler must not consider such a profile healthy merely because the weekly
fraction < 1.

`official` The quota is shared across all devices and API keys of the account. `decision` → the
quota authority is `user_id`, not the key; multiple keys of one subscription are one subject.

### 5.3 Choosing the ledger model

`decision` Per §10.1, KIMI is **not** a GPT-like dual-ledger provider, despite having native
units. The decisive difference: GPT publishes the native charge **per turn**, while KIMI delivers
the native consumption **only as a window aggregate** in `/usages`. There is nothing from which to
build an independent per-turn native ledger, and deriving it by dividing API dollars by a token
price is expressly forbidden.

Hence the actual model — **Claude-like in shape, but with far better resolution**:

1. **API-nanoUSD ledger** — exact, per-turn, from the official rate card §5.1 by **served**
   model. Cumulative per subject.
2. **Native window fraction** — `used/limit` from `/usages`, playing the role of the Claude-like
   quota fraction, but arriving as integers with resolution `FRACTION_SCALE / limit` instead of
   whole percents.
3. **Booster wallet** — real money (cents), a separate third ledger, mixed with nothing.

`decision` **There is no need to estimate the native window capacity — it is published.** `limit`
is the full window size in native units, and `limit - used` is the exact native remainder. Only one
thing is subject to estimation: how much official API replacement cost fits into the window at the
observed load. Therefore the scheme contains neither a native-capacity estimate nor a per-turn
native leg — it has the exact `native_limit_units` and the ordinary §10.5 formula for
`capacity_nanoUSD`.

`unknown` The unit of `used` remains unproven. This does not prevent computing the fraction (the
fraction is dimensionless), but it means the native remainder cannot be converted into tokens or
money until live proof.

`decision` Cohorts (§10.6) are merged only by exact `user_level_name` + exact window
duration. An `unknown` plan blocks cohort aggregation — unlike Claude, here the plan is
machine-readable, so blocking is expected to be rare.

### 5.4 Runtime ordering

`decision` The server starts the first free `/usages` anchor immediately after the `/me` preflight,
then repeats it at `CLAUDE_API_KIMI_QUOTA_POLL_SECS`; roster discovery remains an independent
15-second tick. The poll proceeds sequentially over a snapshot of the current whole-generation
roster and does not return a removed profile back into the generation.

For each subject the ordering is load-bearing:

1. the profile must be idle; the poll records the monotonic generation-start epoch;
2. the known bounded turn FIFO is fully drained, otherwise no HTTP is executed at all;
3. after `/usages` the epoch and in-flight are checked again; any turn that started during the GET
   invalidates the entire snapshot, with no local queue and no customer-concurrency limiting;
4. under the FIFO barrier another drain is performed; the serial PostgreSQL writer reads the
   cumulative official API spend and, for each independent window, performs the immutable
   observation + estimator CAS; a single-turn conflict quarantines only that event, and the
   transient head is held;
5. the runtime publishes the tightest used fraction and full-window cooling until the exact reset
   only after durable success of **all** windows. A DB/CAS/parser/upstream failure preserves the
   last-good quota.

Shutdown closes admission and steady maintenance, waits for the stream finalizers, then repeats the
same turn-before-quota ordering. The final provider read is bounded by the already-existing process
deadline; its cancellation does not allow the old maintenance task to write data after the common
billing flush. No rotating OAuth refresh is started under the deadline: the final poll uses only a
still-valid access token, while refresh/reseal remains an indivisible steady-state operation.

## 6. What remains unproven

Below is the complete list of `unknown`s, each of which fails closed and is cleared only by a
controlled live run on our own subscription:

1. The exact auth header of the Anthropic route.
2. The form of terminal usage and the presence of cache legs in the response.
3. Real SSE incrementality (a buffered single frame ≠ stream).
4. The unit of measurement of `used` in `/usages`.
5. Distinguishing 401/403 for "capability above plan" and "quota exhausted".
6. The set and prices of pricing plans.
7. Behavior when the shared monthly membership ceiling is exhausted.
8. The existence and cost of paid **provider-executed** tool/search units on the subscription
   route. Client-side function calling and inline media are no longer part of this unknown:
   they are priced as body tokens and bounded by the existing hold. Only work the provider
   runs on the caller's behalf can bill a unit outside that bound, and only that stays closed.

None of them blocks building the runtime, metering, credential, and calibration scheme —
only the corresponding live gates are blocked (`PROVIDER_ONBOARDING.md` §2).

## 7. Delivery status

The current chain continues from `master` with producer-first checkpoints. The plane is enabled on
the Anthropic slots and published in the unified router catalog; the marketing site still contains
no KIMI row. Production activation of the router entry and live proofs are claimed only where the
table below says so.

| Stage | Artifact | Status |
|---|---|---|
| research / capability manifest | this file | done |
| official rate card | `crates/metering/src/kimi.rs` | done, 18 tests |
| calibration authority (schema) | `crates/registry/migrations_pg/0027_kimi_window_calibration.sql` | done, expand-only, 2 tests |
| observation types | `crates/registry/src/kimi_calibration.rs` | done, 10 tests |
| credential | `crates/kimi-credential` | done, 18 tests |
| calibration estimator | `crates/forward/src/kimi_calibration.rs` | done, 19 tests |
| Auth Bot: device-code protocol | `crates/authbot/src/kimi_oauth.rs` | done, 14 tests |
| Auth Bot: seller wizard | `crates/authbot/src/{bot,kimi_roster}.rs` | done: text proxy input on `km_proxy`, device flow → atomic roster before payout |
| transport / pool primitives | `crates/forward/src/kimi/**` | roster/client/selection/refresh/error/attempt/FIFO/config done |
| durable calibration read/write in PostgreSQL | `crates/registry` | done; real-PG replay/conflict/CAS/history matrix is green |
| server: env/config | `crates/server/src/config.rs` | done: strict default-off input → typed config |
| server/forward: gateway + readiness | `crates/{server,forward}` | done on mock gates: exact internal dispatch, `/me`, refresh, rotation, stream lifecycle, reserve/delivering/settlement/FIFO |
| last-good roster reload | `crates/{server,forward}` | done on mock gates: 15-second discovery, whole-generation validation, `/me` admission, exact-Arc reuse, refresh-race verification, safe removal |
| quota observations | `crates/{server,forward}` | done on mock/real-PG gates: idle `/usages`, generation-epoch rejection, turn-before-quota drain, exact spend read, independent-window immutable write/CAS, publish-after-durable and bounded shutdown |
| observability, alerts, admin projection | `crates/{forward,server}`, `observability/**`, `docs/ops/MONITORING.md` | done: extended operational status, admin-only `GET /kimi-subs`, fixed-cardinality aggregate metrics, `kimi-provider` alerts with runbook and consistency test |
| blue-green | `systemd/claude-api-kimi{,@}.service`, `deploy/**`, `observability/prometheus/prometheus.yml` | done: two slot units 8804/8805 + stable loopback origin 8803, capability marker, rollback branch stops all incarnations, scrape target `provider: kimi`, `ProviderMode::Kimi` with fail-closed `/v1/messages`; plane **enabled** by the reviewed argv pin `CLAUDE_API_KIMI_ENABLED=1` after live evidence 2026-08-04 |
| safe live-runner | `tools/kimi_calibration/`, `docs/ops/KIMI_CALIBRATION.md` | done offline: dry-run by default, aggregate ceiling $0.0001, exact request-id attribution via admin-only headers, 43 offline tests; paid runs only with explicit permission |
| live matrix on our subscription | — | Vivace subscription connected 2026-08-04; smoke (`/me`, `/usages`, one minimal generation with exact metering) passed; full matrix awaits budget permission and the weekly quota reset |
| router publication | `crates/server/src/router_catalog.rs`, `crates/router/src/{catalog,policy,routing,presets}.rs`, `crates/router/routing-presets.json` | done: internal discovery producer, fourth optional catalog plane over the Anthropic lane, `kimi` arm in the router pricing producer, five aliases advertised |

The next producer-first step is a controlled live run through the live matrix on the connected
Vivace subscription (after its weekly quota resets and budget permission), then the commerce/
OpenKeys and client-documentation surfaces.

## 8. Sources

All links reviewed 2026-08-03.

- `https://platform.kimi.ai/docs/pricing/chat`, `.../chat-k3`, `.../chat-k27-code`, `.../chat-k26`
- `https://platform.kimi.ai/docs/models`
- `https://www.kimi.com/code/docs/en/kimi-code/models.html`
- `https://www.kimi.com/code/docs/en/kimi-code/membership.html`
- `https://www.kimi.com/code/docs/en/kimi-code/error-reference.html`
- `https://www.kimi.com/code/docs/en/third-party-tools/claude-code.html`
- `https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/providers.html`
- `github.com/MoonshotAI/kimi-code` @ `75395f6abb17f83f30d16b51f4e060a639f43622`, MIT
  (read-only; the temporary clone was deleted after the research)
