# KIMI production parity audit — 2026-08-09

Question asked: is KIMI fully in production at every stage, and no worse than any other pool?

Answer, as amended 2026-08-10: **yes.** The one axis that failed — strict-policy admission — was
closed by the pricing dismantling that landed overnight ("one price, one settlement, no retired
pricing left"). `strict_policy` no longer exists in the engine, and with it went the KIMI refusal.
Verified with a real customer key: `kimi/k3` and `kimi/kimi-for-coding-highspeed` both served.

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
| 12 | Strict-policy admission | n/a — concept removed 2026-08-10 | served | parity |
| 13 | Live coverage matrix | n/a | all 4 subscription model ids; 12/12 legs × 2 of 3 profiles | models complete, profiles quota-bound |
| 14 | Paid tool/search units | proved for others | unproved, recorded unavailable | not parity, by contract |

## Axis 12 — closed by the pricing dismantling, not by a fix here

Until 2026-08-10 `KimiGateway::handle` refused a strict key before any pricing was attempted, and
that refusal was correct for its time: removing it on 2026-08-09 produced a live regression,
because the strict reserve writer demanded a `policy_v1` admission snapshot that only a
catalog-backed policy could build. Without the guard the request reached PostgreSQL and died with
`strict reservation lacks a policy_v1 admission snapshot`, surfacing as a retryable `529`.

The overnight commits (`9f3f65d0`, `ca3dd6c9`, `0c236aa2`, `a6e7f2e7`) removed the strict policy
concept entirely — `strict_policy` no longer appears in `crates/forward` or `crates/server`, and
the KIMI refusal went with it. So the axis did not need the catalog entry I had scoped for it; it
needed the design above it to be finished. Recorded here because the earlier analysis was correct
about the mechanism and wrong about the remedy.

Live confirmation on a real customer key, 2026-08-10: `kimi/k3` → served `k3`;
`kimi/kimi-for-coding-highspeed` → served and answered.

## Axis 13 — coverage

Two of three profiles are fully covered: 12/12 legs each, all four served model families, both
context modes including the 1M window, every accepted reasoning effort, $0.0105 per profile.

The third (`kimi-3e80dc1f94df83de`) is at its weekly wall (100/100) and the runner refuses it
before dispatch, spending nothing. It is unavailable, not skipped. The command is in
`research/KIMI_PLANE_PROGRESS.md`.

**Model coverage is complete.** Moonshot's own subscription documentation
(`kimi.com/code/docs/en/kimi-code/models.html`, read 2026-08-09) states that Kimi Code "offers two
models — Kimi K3 and Kimi K2.7 Code — across four model IDs": `k3`, `k3-256k`, `kimi-for-coding`
and `kimi-for-coding-highspeed`. Those are exactly the four the matrix exercised, so every model
the subscription can address is covered.

`kimi-k2.6` has **no subscription model ID** — it cannot be requested by name, which is why no
alias points at it. It exists in `crates/metering` as a tariff key carrying the official API rate
used to price replacement cost. Moonshot documents it as reachable only implicitly ("K3 / K2.7
without Thinking routes to K2.6"), and our own turns contradict that on this route: the `off` legs
priced under the K3 card (`moonshot/kimi/kimi-k3/v2`), not the K2.6 one. Either way there is
nothing to publish and nothing to test.

Two quota facts from the same page explain the consumption numbers above and are worth carrying:
`k3` costs about twice the quota of `k3-256k` for equivalent work inside 256k, and HighSpeed
trades 3× quota for 6× speed. The 1M window requires an Allegretto plan or above; all three of our
profiles are Vivace, so it is available on all of them.

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
