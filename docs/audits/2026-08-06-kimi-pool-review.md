# KIMI pool audit — 2026-08-06

Full review of the KIMI plane against the three references that have already been hardened:
Claude (`crates/forward/src/proxy.rs`), GPT/Codex (`crates/forward/src/codex/`) and Gemini
(`crates/forward/src/gemini/`). Scope: routing, cooling, auth, billing authority, delivery
contract, observability. Evidence is code references plus the live production snapshot.

## Live state at audit time

The plane is enabled and serving from the blue-green slot on port 8804 (scraped as `8803`):

```
claude_api_kimi_enabled                 1
claude_api_kimi_profiles                3
claude_api_kimi_live_profiles           3
claude_api_kimi_available_profiles      1     ← two of three are resting
claude_api_kimi_quota_cooling_profiles  2
claude_api_kimi_auth_quarantined        0
claude_api_kimi_transport_cooling       0
claude_api_kimi_calibration_persistence_ok 1
```

Note for operators: the inactive `claude-api-kimi@8805` slot also publishes the series with zeros.
Read KIMI gauges with an explicit instance filter or a stopped slot reads as an outage.

## Parity already achieved

These were the expensive lessons on the other planes, and KIMI has them:

- **Three-axis profile state** — `ProfileEffect::{AuthQuarantine, CoolUntilReset, TransportFault}`
  (`kimi/pool.rs:58`), the same separation of credential / capacity / transport the cooling
  invariant requires. `CoolUntilReset` is explicitly documented as capacity, not health.
- **Single-flight token refresh** with a rejected-token comparison (`kimi/gateway.rs:943,1069`),
  so a concurrent 401 burst performs one refresh rather than one per request.
- **Rotation budgets separated** — auth and quota rotate without spending the transport budget
  reserved for genuine outages (`kimi/transport.rs:108`).
- **Affinity / cache-root** participation (`kimi/gateway.rs:1277`).
- **Exact calibration** (`crates/forward/src/kimi_calibration.rs`, `crates/metering/src/kimi.rs`)
  with persistence health exported.
- **Sealed roster** with atomic republish by the authbot, rescanned without an engine restart.
- Customer-facing errors run through `local_err_for`, so KIMI inherits the `not_started` refund
  proof and the `x-request-id` header added for the Anthropic plane.

## Gaps, ranked

### 1. KIMI does not exist in the pricing authority

`SnapshotProvider` admits only `anthropic`, `openai`, `google`
(`crates/registry/src/pricing/snapshots.rs:54`). KIMI reserves through the legacy path
(`kimi/gateway.rs:1671`, `reserve_request_for_execution`) and has no catalog entry in any
capability generation.

Consequences today: KIMI revenue is priced by the account's legacy multiplier and never resolves a
release-v2 policy, so B2C/B2B class, product scope and the catalog gate simply do not apply to it.
Consequence tomorrow: the moment KIMI is put behind release-v2 it fails exactly the way the
pinned-catalog incident did — a routable model no authority can quote.

The coverage gate `advertised_models_all_have_an_exact_tariff` does not cover KIMI either, because
`routing-presets.json` carries no `kimi/*` entries. That is correct while the plane is unpublished
and must be extended in the same commit that publishes it.

### 2. No smooth-wait window

Claude, Gemini and now Codex all spend `CLAUDE_API_SMOOTH_WAIT_MS` before turning a momentary
capacity gap into a customer error. KIMI refuses immediately. With two of three profiles resting
right now, an empty-pool moment is not hypothetical.

### 3. `429` is classified as a transport fault

`kimi/transport.rs:102` maps `408 | 409 | 425 | 429 => Transport`. Every other plane treats 429 as
a quota verdict: cool from `Retry-After`, rotate, do not spend the transport budget. Here a rate
limit burns the outage budget and can escalate to `TransportFault`, whose remedy is rebuilding the
client — the wrong response to being told "slow down".

### 4. `403` collapses into quota

`kimi/transport.rs:101` maps `403 => QuotaExhausted`, so the profile rests until a "reset" that a
permission refusal will never produce. The Gemini incident of 2026-08-06 was exactly this shape:
a request-scoped `403` read as an account state, which rested the fleet and produced a retryable
error for an input no retry could fix. KIMI's own 403 semantics have not been established — that
is the first thing to determine, not to assume.

### 5. Cooling is per profile, never per model

Gemini cools model×profile (`cool_model_until`) precisely so one degraded model cannot withdraw a
whole subscription. KIMI has one cooling axis, so any model's failure removes the profile for all
models.

### 6. Observability is roughly a fifth of its peers

19 exported metric lines against 97 for Gemini and 105 for Codex; 5 alert rules against 13 for
Codex. The plane touches `Metrics` nowhere in `kimi/gateway.rs`: there is no request counter and no
upstream 401/429/5xx counters, so **an error rate for KIMI cannot be computed at all** — the gap
that made the Gemini investigation take a day. Nothing alerts on `available_profiles` collapsing to
one, which is the live state.

## Recommended order

1. Establish KIMI's real `403` and `429` semantics against the provider, then fix the
   classification (items 3 and 4). Cheap, and it is the difference between resting a subscription
   correctly and resting it for nothing.
2. Add the request and upstream-verdict counters (item 6). Without them the next two items cannot
   be measured, and no incident can be attributed.
3. Add the smooth-wait window (item 2), reusing `crate::proxy::smooth_step` and the discipline used
   on Gemini and Codex: wait only when the round collected no provider verdict.
4. Per-model cooling (item 5).
5. Pricing authority (item 1) — a full capability generation, and the largest piece. It is last
   because it is only required before KIMI is sold, not before it is stable.

Items 1–4 are prerequisites for calling this plane production-ready; item 5 is a prerequisite for
selling it.
