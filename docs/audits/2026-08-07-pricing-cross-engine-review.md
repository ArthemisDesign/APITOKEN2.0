# Pricing cross-engine audit — 2026-08-07

Written after the generation-6 cutover, to answer three questions with evidence rather than
opinion: can this migration still break things, where exactly can the commerce↔engine handover
fail, and would moving pricing into the engine be better.

## 1. Live state after the cutover

Catalog head `main`/`openkeys` = generation 6 (activated 2026-08-06 16:08 UTC). Release head = 41,
pinning capability 6 and catalog 6 (19:26 UTC).

Every queue that can change what a customer is charged is fully settled:

| Queue | State |
|---|---|
| `engine_pricing_jobs` | 10 confirmed |
| `engine_catalog_jobs` | 8 confirmed |
| `engine_switch_jobs` | 4 confirmed |
| `pricing_release_control_jobs_v2` | 20 confirmed |
| `engine_policy_jobs` | 475 confirmed, 6 superseded, **2 dead** |

Nothing is pending, nothing is retrying, nothing is stuck mid-flight. The authoritative half of the
migration landed cleanly.

The **evidence** half did not:

| Queue | State |
|---|---|
| `pricing_shadow_policy_jobs_v2` | 510 confirmed, **2181 blocked** |
| `pricing_shadow_rollouts_v2` | 3 confirmed, **9 blocked** |
| `pricing_stage8_capture_jobs_v2` | 2 passed, **6 blocked** |

## 2. What the blocked work actually is

All 2181 blocked shadow jobs are OpenKeys accounts, blocked for three reasons:

```
1506  shadow policy prepare rejected with locked                        (06.08)
 424  shadow policy activation rejected with missing_dependency          (04–06.08)
 251  locked OpenKeys active policy drifted from the durable expectation (04.08)
```

This is a race, not corruption: the live rollout held the account policies locked while the shadow
lane tried to prepare its comparison against the same accounts. The queue stopped growing after the
cutover finished — nothing new on 07.08.

**Customer impact: none.** The shadow lane is a dry-run that compares what the new policy *would*
charge against what the live one does. It never charges anyone.

**Real cost: reduced evidence.** For those OpenKeys accounts the comparison never ran, so
generation 6 was activated with the shadow safety net absent for that class. The rollout was not
verified to the standard the design intends — that is the finding, and it is worth knowing before
the next generation goes out.

The 2 dead `engine_policy_jobs` are benign leftovers: both target one b2b account with
`version_conflict`, and that account now carries a healthy release-v2 assignment (policy version 3).
The legacy lane tried to write a policy version that release-v2 had already superseded.

## 3. Where the handover can break

Pricing is authored in commerce (TypeScript) and enforced in the engine (Rust). They share no
database: everything crosses through durable jobs and the Control API. That boundary has four
distinct failure classes, and this week produced an example of three of them.

**A. Authored but never admitted.** A model is routed and metered but has no catalog entry, so
admission cannot quote it. Cost this week: 953 customer-visible `529`s in a single day, plus a
`gpt-image-2` that was priced in code and in generation 6 while the head still pinned generation 5.
This is the most expensive class because nothing fails until a customer asks for that model.
Detection is now partly automated (`advertised_models_all_have_an_exact_tariff`), but that gate
checks `crates/metering` only — it does **not** check that a catalog entry exists, which is exactly
the half that failed. Closing that is the single highest-value follow-up in this document.

**B. Delivered but not confirmed.** A job reaches the engine and its acknowledgement does not come
back, or comes back against a superseded expectation. Observed as the 2 dead policy jobs and the
`missing_dependency` shadow blocks. The two-phase prepare/confirm protocol is what keeps this from
turning into divergence, and it worked: no queue is in an ambiguous state.

**C. Two writers, one account.** The live rollout and the shadow lane both act on the same account
policy. Observed as 1506 `locked` rejections. Contained by locking, at the cost of losing the
shadow evidence.

**D. Runtime fallback gaps.** The engine's own reaction when pricing is unavailable. This produced
the week's other regression: closing the main admission path routed unpriced models into a second
branch (`pricing_release_cutover_retry_failed`) that treats "no active release" as a refusal, so
one b2b account still receives ~2 × `529` per hour. Note this class is **not** cross-engine at all —
it is Rust-side logic, and it would exist unchanged if pricing lived entirely in the engine.

## 4. What the split buys

The separation is not decoration. Three properties depend on it:

- **Money authority stays in one place.** The engine owns balances, reservations and the ledger;
  commerce can never write them, only ask through the Control API. A bug in the storefront cannot
  mis-charge anyone.
- **Pricing changes without an engine deploy.** A discount, a policy, a rollout is data delivered
  through a queue — no Rust rebuild, no blue-green restart of the request path. That matters when
  the request path is also the thing that must not go down.
- **Rollout is atomic and reversible.** An immutable release plus a single head switch means the
  whole fleet moves at once and can be pointed back. Per-account migration would create the mixed
  state the design explicitly rejects.

## 5. Would moving pricing into the engine be simpler?

Measured, not guessed:

| | Commerce side | Engine side |
|---|---|---|
| Pricing code | ~22 300 lines | ~21 100 lines |
| Pricing tables | 34 | 22 |
| Delivery queues | 8 | — |

Merging would delete a real, bounded amount of machinery: the 8 queues, the prepare/confirm
protocol, the shadow-delivery lane, and failure classes **B** and **C** entirely — roughly the
2200 blocked rows above stop being possible. That is not nothing.

It would not touch classes **A** and **D**, which caused every customer-visible failure this week.
A model still has to be admitted to a catalog before it can be quoted, and the runtime still has to
decide what to do when it cannot quote. Those are the expensive bugs, and they are indifferent to
where the code lives.

Against that, three things get worse:

- **Every price change becomes a deploy** of the request path. Today's cadence was five blue-green
  switches in three hours; adding pricing edits to that stream increases the number of times the
  serving path restarts for a reason unrelated to serving.
- **The admin surface still needs a boundary.** The panel and the operator API are TypeScript and
  would keep talking to the engine over HTTP — so the protocol does not disappear, it moves.
- **The engine takes on business logic it deliberately excluded.** `CLAUDE.md` states the invariant
  plainly: the commercial layer never opens the engine database, and the only crossing is the
  Control API. Merging pricing inverts that, and the reason it exists — one authority for money —
  is the property most worth keeping.

**Assessment.** The split is not what is hurting. The pain is the number of *stages* inside it —
generations, shadow lanes, Stage 5/6/8/9 — not the fact that two systems talk. Collapsing stages
and closing the catalog-coverage gate would remove more real risk than a merge, at a fraction of
the cost and without giving up single-authority money.

## 6. Recommended order

1. Extend the coverage gate to assert a **catalog entry**, not only a `metering` tariff. This is
   the class that reached customers 953 times in a day and it is currently ungated.
   *Done — `packages/db/src/pricing-catalog-coverage.test.ts`. The four models unpriced today are
   recorded as a self-expiring exemption so the gate could land before generation 7 closes them.*
2. Land generation 7 for `claude-opus-4-6`, `claude-opus-4-5`, `claude-sonnet-4-5`. That closes the
   remaining `529`s, including the regression in class D.
3. Re-run or explicitly retire the 2181 blocked shadow jobs before the next generation, so the next
   rollout is not the second one without its evidence.
   *Path: once the classifier fix below is deployed, an operator stages a fresh shadow rollout
   through the existing admin surface (`stagePricingShadowRolloutV2`). The blocked rows stay as
   history — they record what happened — and the new rollout re-runs the comparison. No new code.*
4. Decide whether the shadow lane should be allowed to run against accounts the live rollout holds
   locked. Today it silently loses; either it waits, or it should not be scheduled during a cutover.
   *Done — it now waits. `requireMutation` treated every engine rejection as permanent, so `locked`,
   `missing_dependency` and `stale` — all of which clear as the rollout proceeds — were recorded as
   terminal. They are retried now; a rejection that states a verdict still blocks.*
