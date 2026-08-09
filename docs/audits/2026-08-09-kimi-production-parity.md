# KIMI production parity audit — 2026-08-09

Question asked: is KIMI fully in production at every stage, and no worse than any other pool?

Answer: **on the legacy-policy path, yes. On the strict-policy path, no** — and since new accounts
are provisioned strict and the existing cohort is being backfilled, that gap decides whether the
provider is sellable, not whether it works.

Every "yes" below was established by a request, not by reading code. Two claims in this file's
predecessors were established by reading code and both were wrong; the corrections are recorded at
the end rather than quietly dropped.

## Axes

| # | Axis | Claude / GPT / Gemini | KIMI | Verdict |
|---|---|---|---|---|
| 1 | Router catalog | published | `kimi/*`, 25 refreshes, 0 failed, 0 degraded | parity |
| 2 | Wire protocols | native + universal | `/v1/messages` and `/v1/chat/completions` both answered live | parity |
| 3 | Namespaced ids | stripped by admission | `kimi/<alias>` resolved to the bare alias (`resolve_public_model`) | parity |
| 4 | Price authority | hot tariff overrides | `moonshot/kimi/*` seeded; live turns pin `…/v2` | parity |
| 5 | Settlement exactness | per-leg recomputation | 24 paid turns, every money leg reproduced from token counts | parity |
| 6 | Cooling | provider verdict only, two axes | same, `Ineligible` + escape hatch for environmental reasons | parity |
| 7 | Request/failure counters | per provider | `kimi_requests`/`failures`/`capacity_exhausted` + 2 alerts | parity |
| 8 | Smooth-wait window | 8 s | same window, same `proxy::smooth_step` | parity |
| 9 | Customer usage attribution | own bucket | own bucket in dashboard, OpenKeys and admin | parity (was a defect, see below) |
| 10 | Public rate card | on the pricing page | Kimi panel, rates from the audited card | parity |
| 11 | Client docs | builder + API reference | both, plus Kimi Code and Claude Code guides | parity |
| 12 | **Strict-policy admission** | **served** | **refused, terminally** | **not parity** |
| 13 | Live coverage matrix | n/a | 12/12 legs × 2 of 3 profiles | partial, quota-bound |
| 14 | Paid tool/search units | proved for others | unproved, recorded unavailable | not parity, by contract |

## Axis 12 — the one that matters

`KimiGateway::handle` refuses a strict key with `kimi_strict_pricing_unavailable` before any
pricing is attempted. This is not a leftover: removing it on 2026-08-09 produced a live regression,
because there is a second gate behind the obvious one. The strict reserve writer demands a
`policy_v1` admission snapshot carrying a rule identity (`provider`, `rule_id`, `rule_digest`,
`rule_scope`, `pricing_mode`), and `SnapshotProvider` has no `kimi` variant. Without the guard the
request reached PostgreSQL and died with `strict reservation lacks a policy_v1 admission snapshot`,
surfacing as a `529` with `Retry-After` — a retryable error for a permanently deterministic
condition. Restored the same day; production verified back to a terminal `404`.

This is **not** the position Gemini is in. Gemini resolves its release path first and serves the
strict account from it.

What remains undecided is whether a hot policy rule scoped to provider `kimi` is sufficient, or
whether `gate_lineage` also requires a catalog entry. That is an owner decision about the pricing
catalog and one cheap reversible experiment, not another gateway edit.

## Axis 13 — coverage

Two of three profiles are fully covered: 12/12 legs each, all four served model families, both
context modes including the 1M window, every accepted reasoning effort, $0.0105 per profile.

The third (`kimi-3e80dc1f94df83de`) is at its weekly wall (100/100) and the runner refuses it
before dispatch, spending nothing. It is unavailable, not skipped. The command is in
`research/KIMI_PLANE_PROGRESS.md`.

`kimi-k2.6` is absent from the matrix because no published alias reaches it: it is a tariff key
carrying the official API rate used to price replacement cost, and the subscription route
publishes no id for it. The widely repeated claim that a thinking-off request reroutes K3 down to
K2.6 is contradicted by our own turns — the `off` legs priced under the K3 card.

## Defects this audit surfaced, and their fixes

- **OpenKeys reported KIMI spend as Claude spend.** `usageProviderOf` falls through to a
  catch-all, and the catch-all is not "unknown" — it is `claude`. Fixed; the label map now lives
  next to the type so the compiler requires an entry.
- **The dashboard could never show KIMI.** Cards are built from the applied pricing policy, which
  by design does not describe a model outside the pinned catalog. Providers in that position are
  now shown, marked ready, and labelled with the account multiplier settlement actually applies.
- **The published router id could not be walked.** The plane stripped only `anthropic/`, so
  `kimi/k3` matched no alias and went verbatim to the Claude upstream — every client that took the
  id from `/v1/models` failed on its first request.
- **The Kimi Code guide emitted one model.** The harness picks from `[models.*]`, so users got a
  single choice; it now declares the provider's whole catalogue.
- **The live runner failed on a tariff rename**, not a price difference: it demanded the compiled
  schedule id while settlement pins a hot override.

## Corrections to earlier claims in this repository's docs

1. "KIMI's strict refusal mirrors Gemini's" — wrong. Gemini serves strict accounts.
2. "A full coverage matrix costs 3 weekly quota units" — wrong; that summed only resolved deltas.
   The profile's own counters moved 53 → 78 weekly and 11 → 61 on the 5h window.
3. "`/v1/models` on the subscription route is ungated" — no longer true; it returns an
   authentication error. The conclusion (unusable as a health probe) survives for the opposite
   reason.
