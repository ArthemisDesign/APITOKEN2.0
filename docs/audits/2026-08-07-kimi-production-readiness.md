# KIMI production-readiness review — 2026-08-07

Written against `docs/engine/PROVIDER_ONBOARDING.md` and the Claude/Codex/Gemini implementations,
after reading `research/KIMI_PLANE_PROGRESS.md` first as that contract requires. The question is
not "does it work" — it serves traffic today — but "what stands between the current backend-only
plane and selling it".

## Where the plane actually is

Enabled in production since `0de59e28` (2026-08-04) as a **backend-only** plane: no public
hostname, no router namespace, no catalogue entry. Live state at review time:

```
claude_api_kimi_enabled                    1
claude_api_kimi_profiles                   3
claude_api_kimi_live_profiles              3
claude_api_kimi_available_profiles         1     ← two rest on quota
claude_api_kimi_quota_cooling_profiles     2
claude_api_kimi_auth_quarantined_profiles  0
claude_api_kimi_transport_cooling_profiles 0
claude_api_kimi_calibration_persistence_ok 1
claude_api_kimi_calibration_pending_events 0
```

The chain the onboarding contract lists — research, metering, migration, credential, estimator,
Auth Bot protocol and wizard, transport/pool, durable calibration, server wiring, observability,
admin board, live runner — is complete and each link cites a landed SHA in the ledger. What remains
is not construction.

## Model coverage: complete and fail-closed

Four official models with effective-dated schedules, five subscription aliases mapping onto them:

| Alias on `api.kimi.com/coding` | Official tariff key | Accepted context |
|---|---|---|
| `kimi-for-coding` | `kimi-k2.7-code` | 256K |
| `kimi-for-coding-highspeed` | `kimi-k2.7-code-highspeed` | 256K |
| `k3`, `k3[1m]` | `kimi-k3` | 1M |
| `k3-256k` | `kimi-k3` | 256K |

`k3[1m]` is correctly modelled as the Claude Code spelling of the same tariff at a wider context,
not as a separate model. Retired platform models (`kimi-k2-*`, `kimi-latest`,
`kimi-thinking-preview`, `moonshot-v1-*`, `kimi-k2.5`) are deliberately absent so an absent entry
fails closed at reserve. This is the standard the other planes hold and it is met.

## What blocks selling it

Four gaps, in the order they must be closed. Note that none of them is a defect in the plane: they
are the difference between a plane that serves and a product that can be sold.

### 1. KIMI does not exist in the pricing authority

`SnapshotProvider` admits `anthropic`, `openai`, `google` — and nothing else
(`crates/registry/src/pricing/snapshots.rs`). The plane reserves through the legacy path
(`kimi/gateway.rs`, `reserve_request_for_execution`), so KIMI traffic never resolves a release-v2
policy: no account class, no product scope, no catalog gate.

Today that is harmless because no customer is routed here. The moment one is, KIMI is billed by the
account's legacy multiplier while every other provider is billed by its release policy — and the
first attempt to put it behind release-v2 reproduces the pinned-catalog failure that refused 953
customer requests in a day on 2026-08-06.

This is the largest piece of work and it gates revenue, not stability.

### 2. Not discoverable

`crates/router/routing-presets.json` carries no `kimi/*` entry, so the unified `/v1/models` never
advertises it and no client can find it. That is correct while the plane is unpublished, and it must
be added in the same commit that publishes it — the catalog-coverage gate added on 2026-08-07
(`packages/db/src/pricing-catalog-coverage.test.ts`) will fail the build if a `kimi/*` model is
advertised without a catalog entry, which is exactly the intended interlock.

### 3. No smooth-wait window

Claude, Gemini and Codex all spend `CLAUDE_API_SMOOTH_WAIT_MS` before turning a momentary capacity
gap into a customer error. KIMI refuses immediately. With **one of three profiles available right
now**, an empty-pool moment is not hypothetical — it is the expected state on a subscription whose
weekly quota is nearly spent.

The fix is mechanical and the pattern is established three times over: wait only when the round
collected no provider verdict, clear the tried set between rounds, keep a real rejection terminal.

### 4. No request or verdict counters

`crates/forward/src/kimi/gateway.rs` never touches `Metrics`. The plane publishes aggregate gauges
— profiles, live, available, cooling, in-flight, calibration health — and five alerts, which is why
the earlier claim that its observability is "roughly a fifth of its peers" was about volume, not
absence. But there is no request counter and no upstream 401/429/5xx counters, so **an error rate
for KIMI cannot be computed at all**. That is the same blindness that made the Gemini investigation
of 2026-08-06 take a day, and it should be closed before customers arrive rather than after.

## Corrections to the 2026-08-06 pool review

That review is superseded on two points, and the corrections matter because both were arguments to
change working code:

- **`429` classified as transport is correct here.** Kimi's own error reference states
  `429/5xx — inference overload, "retry directly"`; this provider signals quota through `403`, not
  `429`. The earlier finding applied Google's and Anthropic's semantics to a provider that does not
  share them.
- **`403` collapsed into quota is a documented, deliberate fail-closed choice**, recorded in
  `docs/engine/KIMI_PROVIDER.md` with an explicit `unknown` marker: 403 covers both quota exhaustion
  and missing plan capability, they are distinguishable only from the error body, and until live
  evidence exists the handler rests the profile until reset rather than marking it dead.

The residual risk in the second point is real and worth keeping visible: if a 403 is "the tier does
not grant this capability", it describes the request, not the account, and resting the profile until
reset removes capacity for nothing. That is the same shape as the Gemini incident. It is resolved by
capturing the error body during the live matrix, not by reclassifying on a guess.

## The one thing that is genuinely blocked

The paid live matrix. It needs two things from a human, and neither is engineering work:

1. **Budget authorization above $0.0001.** At the current ceiling no leg passes its worst-case
   full-context bound, so the runner fail-closes before dispatch — verified by dry run against
   production on 2026-08-04: a 12-leg plan, `paid_requests: 0`. This is the guard working, not a
   defect.
2. **Quota headroom.** Two of three profiles rest on quota right now. The matrix needs a profile
   with room to spend.

Until it runs, these stay unproven and every one of them is a `unknown` in the manifest: incremental
SSE behaviour, quota-movement pairing, the 401/403 split above, and per-model consumption weights.
The plane is therefore **preview**, not GA, and saying otherwise from green mock tests is precisely
what the onboarding contract forbids.

## Recommended order

1. Counters (gap 4) — cheap, and nothing else can be measured without them.
2. Smooth-wait window (gap 3) — mechanical, three existing implementations to copy.
3. Live matrix — needs the human decisions above; converts four `unknown`s into evidence.
4. Pricing authority (gap 1) — the revenue gate, and the largest piece.
5. Catalogue publication (gap 2) — last, and only after 1–4, because it is the step that exposes
   KIMI to customers.
