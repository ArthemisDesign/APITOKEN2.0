---
name: provider-onboarding
description: Drive a subscription-backed AI provider from research to verified production GA in Claude_API, including credential/Auth Bot, provider runtime, exact billing, Claude/GPT-grade evidence calibration, safe live load tests, and the compact subscriptions admin UI. Use when adding a new provider, a distinct provider plane, subscription quota/credit accounting, or bringing an incomplete Claude/Codex/Gemini-style integration up to the repository's current standard.
---

# Provider onboarding

Treat `docs/engine/PROVIDER_ONBOARDING.md` as the terminal contract. Do not stop at a working
request, mock test, merged runtime, or visually plausible capacity number.

## Start from current authority

1. Create an isolated task worktree from current `origin/master` and follow `AGENTS.md`.
2. Read `CLAUDE.md`, `BRANCHES.md`, `CONTRIBUTING.md`, `docs/CHANGE_CHECKLISTS.md`,
   `docs/DEPENDENCIES.md`, and every local `CLAUDE.md` for touched components.
3. Read `docs/engine/PROVIDER_ONBOARDING.md` completely, then
   `docs/engine/PROVIDER_WIRING_CHECKLIST.md`, which maps the same contract onto the exact files,
   symbols, commit order and already-encountered traps. Commit something within the first minutes:
   a worktree with no commits reads as clean+merged and is reaped automatically.
4. Compare the current Claude and GPT implementations before designing provider-specific code:

   - `crates/forward/src/anthropic_calibration.rs`;
   - `crates/forward/src/codex/calibration.rs`;
   - `crates/registry/src/provider_calibration.rs` and the PostgreSQL equivalents;
   - `crates/metering/src/{lib,codex,gemini}.rs` as applicable;
   - `tools/claude_calibration/{run_live.py,test_run_live.py}`;
   - `docs/ops/CLAUDE_CALIBRATION.md` and `docs/engine/CODEX_PROVIDER.md`;
   - `apps/admin/src/app/subscriptions/{fleet-capacity-overview,provider-board-ui,claude-capacity-board,codex-capacity-board}.tsx`;
   - `apps/admin/src/app/subscriptions/page.test.tsx` and `docs/product/ADMIN_PANEL.md`.

If a named path moved, find its current replacement with `rg`; never reconstruct behavior from an
old commit or this skill alone.

## Do not hand back a half-built provider

The deliverable is a provider that works end to end, not a set of green tests. A merged commit, a
passing suite, a sealed credential or an operator button is progress, never completion. Keep going
until the terminal state is reached: an operator can acquire a subscription through the Auth Bot,
the resulting profile becomes routable capacity, traffic is billed against the official rate card,
and calibration records evidence from real quota movement. If a live subscription is missing, build
and prove everything that does not need one, then report exactly which live gate is blocked and what
the human must do — that is the only acceptable stopping point short of the terminal state.

Concretely, before writing any code, write down the whole chain to that terminal state and keep it
visible: research, metering, migration, credential, estimator, Auth Bot protocol, Auth Bot wizard,
transport/pool, durable calibration, server wiring, observability, live runner. Then work it without
waiting to be asked for the next link. Finishing one link and handing back is the failure mode this
section exists to prevent.

**This does not mean one giant merge.** Schema and dependent code must never travel together: the
migration lands alone and its `deploy/migration` plus `deploy/watchdog` must be GREEN before
anything reads or writes those tables. Producer-first ordering and small reviewable commits are what
make an unattended chain safe to land — chain the merges autonomously, do not collapse them. An
agent that merges a schema together with its first reader has not been fast, it has removed the
rollback path.

Two failure modes to name explicitly, because both look like success:

- **Stopping at the seam.** Product buttons without the handler that completes the deal, or a sealed
  profile with no plane that reads the roster, leave a provider that appears wired and serves
  nothing. If a change creates such a seam, say so in the commit body and close it in the next one.
- **Declaring done from green tests.** Mock coverage proves guards, never the provider contract.
  State preview versus GA explicitly, and list every unknown that still blocks its own live gate.

## Plan to a terminal outcome

Track explicit phases: research/capability manifest, credential and Auth Bot, transport/pool,
metering/settlement, durable calibration, admin/product surfaces, observability/deploy, safe live
matrix, and production verification. Mark unknown plan/model/tier capabilities fail closed.

Deliver cross-context contracts producer-first. Deliver every additive database migration in its
own first commit and wait for `deploy/migration` plus `deploy/watchdog` GREEN before dependent code.

Before extending an existing calibration table, audit its complete durable identity. If its primary
key omits a newly proven dimension such as paid plan, provider bucket, duration, service tier, or
tariff schedule, create a new additive authority beside it. Do not mutate the old primary key in
place and do not copy legacy estimates/observations whose missing identity, resolution, source, or
request attribution cannot be proved. Keep the serving legacy writer intact for migration-first
rollout, land the new schema, wait for production migration GREEN, then switch runtime readers and
writers in the dependent change. Delete the legacy runtime path only in a separately verified
cleanup once no deployed SHA needs it.

## Build calibration as an evidence system

Select the estimator shape from provider facts:

- If the provider publishes a native subscription unit, keep native consumption and official API
  replacement cost as separate ledgers, as GPT does. Never derive one from the other.
- If the provider publishes only quota fraction, calibrate the realized official API-price workload
  blend against that fraction, as Claude does. Never invent native credits or a fixed dollar nominal.
- If provider buckets publish amounts with unclear semantics, persist them separately as raw quota
  evidence until live tests prove the unit. Do not divide API dollars by a token price to fabricate
  capacity.

Implement this ordered chain:

1. Price authoritative terminal usage in integer nanoUSD using an effective-dated official tariff
   schedule and disjoint usage legs. Preserve provider-reported tier separately from the accepted
   billing tier.
2. Persist one immutable turn event keyed by an internal request id that survives pre-byte retries.
   In the same transaction, advance cumulative per-subject ledgers. Exact replay is idempotent;
   changed payload under the same id is a typed conflict.
3. Put transient persistence behind a bounded FIFO. Keep a failed head pending, quarantine semantic
   conflicts, flush before any later turn or free quota poll, retry during health sweeps, flush on
   retire/shutdown, and publish pending/dropped/persistence health.
4. Parse provider utilisation as exact fixed-point decimal and preserve the endpoint's actual
   measurement resolution. Persist immutable per-subject, per-plan, per-bucket, per-duration quota
   observations with reset evidence and cumulative ledger totals.
5. Keep the first snapshot as an anchor. Count only positive movement paired with positive settled
   spend/consumption. Hold quota that arrives before settlement, expose repeated quota-only movement
   as unattributed, and never let reads write evidence.
6. Handle reset/rollback, timestamp jitter, rolling windows, stale/duplicate observations, cutovers,
   estimator upgrades, overflow, and unbounded uncertainty explicitly. Rebuild changed estimator
   versions from immutable history.
7. Compute with checked integer/rational math only. Publish capacity, remaining, low/high bounds,
   samples, observed fraction/spend, resolution, confidence/maturity, reset, source, and coverage.
   Unknown is `null`, never zero; an unbounded high is `null`, never a guessed ceiling.
8. Pool only like-for-like cohorts: exact paid plan + exact native duration/bucket/schedule. Keep
   individual evidence for audit. Do not pool workload-dependent API-dollar capacity as a universal
   promise when native mix changes its value.

Calibration is acceptable only when deterministic tests prove the first interval, quantisation
envelope, mixed workloads, settlement lag, unattributed movement, reset/rollover, jitter, cutover,
history rebuild, replay conflict, FIFO recovery, independent durations, exact remaining, invalid
state, and overflow failure.

## Write a safe live calibration runner

Add `tools/<provider>_calibration/run_live.py`, its offline unit tests, and an ops runbook. Model it
on `tools/claude_calibration`, but derive model/tier/token/tool coverage from provider-native facts.

Require all of these safeguards:

- dry-run by default and explicit `--execute` for paid traffic;
- exact integer nanoUSD budget, a hard CLI maximum no greater than the user's authorized limit,
  and worst-case preflight before every paid request. Encode the authorized scope literally: use
  both per-profile and aggregate guards when the limit is per subscription, or one aggregate guard
  when the user authorized one total run budget; never silently multiply a total limit by fleet size;
- an exact target profile/session that cannot spill or rebind to a neighbour; preserve real hard
  quota walls, cooling, dead auth, and provider denial;
- baseline delivery health (`pending=0`, `dropped=0`, persistence/authority healthy) and authoritative
  paid-plan identity before traffic;
- a unique run id and isolated cache keys; share only the intended write/read cache pair;
- persist an incomplete machine-readable checkpoint and resume the same run id, aggregate budget,
  completed outcome set, and cache lineage only when the serving backend returns an authoritative
  execution-not-started proof. A retry hint or sanitized status body is not proof. Never repeat
  completed legs, silently add a new profile/model, or resume a paid request whose outcome is
  transport-ambiguous;
- immutable-event attribution by the exact new request id plus profile/model/tier/full usage vector;
  concurrent unrelated traffic must not affect the record; ambiguity stops fail closed;
- read-only retries only. Never automatically repeat a paid request after an ambiguous transport
  failure;
- enough delay/polling for provider quota resolution and backend debounce. Require a post-turn quota
  observation whose authority timestamp is at or after immutable turn completion before assigning
  its delta to a model/token class; an unresolved snapshot is not zero and is excluded from ranking;
- include provider-owned hidden/system/tool prompt input in the pre-dispatch bound. If the free token
  counter omits any such leg and no smaller provider-enforced ceiling is proved, reserve the model's
  complete accepted input-context limit for every affected generation request;
- dispatch a paid tool/search capability only when official or provider-enforced facts prove a
  finite per-request unit ceiling. A conservative fanout guess is not a hard budget guard: record
  the capability as unavailable and spend nothing until the ceiling is proved;
- a full matrix of every supported model, tier, context mode, token class, cache TTL, reasoning/media,
  and billed tool/search unit; record tested unavailability instead of silently skipping it;
- a machine-readable report containing exact spend, before/after fractions, bounds, coverage,
  unavailable capabilities, profile stops, and profitability only for positive observed deltas
  whose profile interval contains no other immutable turn. Exact request attribution alone does
  not make a shared quota delta model-specific under concurrent traffic.

Unit-test the runner's budget/rebind guard, exact attribution amid concurrent traffic, ambiguity,
usage-leg preservation, catalogue/alias ceilings, capability coverage, cache isolation, safe retry
policy, secret containment, incomplete-report behavior, and profitability ordering. Mock tests prove
guards; only owned live subscriptions prove the provider contract.

When applying this workflow to a provider whose quota endpoint is separate from generation (as in
Gemini), settle the terminal usage event first and make the next quota poll flush the turn FIFO
before reading cumulative spend. Preserve the quota decimal's lexical resolution (`0.4` is less
precise than `0.40000000`), record poll observations without inventing a request id, and use a full
opaque admin-only profile id when a short email hint is not collision-proof. If ordinary concurrent
traffic can hit the same profile/model, add a separate canonical admin-only calibration request-id
override and have the runner preselect that immutable id; diffing aggregates or guessing among new
events is not exact attribution. A successful response without terminal usage may retain the bounded
customer hold, but it must not create a synthetic calibration vector.

## Reuse the compact admin capacity design

Keep `/subscriptions` an operator control room, not a calibration laboratory.

1. Add the provider to the unified `FleetCapacityOverview` card row. Show the provider-native 5h/7d
   equivalents only when those exact windows exist; otherwise label its real durations. Each rail
   shows current API-$ remaining, full calibrated window, used share, ready/total identities, and
   measured coverage.
2. Below the overview, render one compact account/profile table with bounded email hint on the left,
   plan and state, quota/reset, and exact remaining/full API-$ for each real window. Reuse
   `ProviderSection`, `ProviderQuotaMeter`, `TableCard`, and the existing CSS language.
3. Render dead/non-routable identities as `вне ротации`; render stale or incomplete evidence as
   `обновляем`/`ждём данные`; never show stale capacity as saleable and never turn `null` into `$0`.
4. Keep model availability as a compact count only when operationally useful. Do not add token
   capacity, profitability, private quota-bucket, raw ledger, schedule, UUID, proxy, or transport
   matrices to the main page.
5. Accept money only as decimal integer strings/BigInt. Prevent duplicate rows from model/window
   joins, keep the account column sticky in wide tables, and verify desktop/mobile overflow.
6. Add SSR/component tests for exact values, privacy masking, ordering, null/stale/dead states,
   duplicate prevention, and explicit absence of removed analytics. Build and visually inspect the
   production page with deterministic fixtures before delivery.

## Finish with production evidence

Run path-appropriate unit, integration, load-runner offline, workspace, docs, shell, and diff gates.
Land only through `deploy/agent-merge.sh`, wait for the exact SHA's `deploy/watchdog` GREEN, then run
the controlled live matrix through the stable production route. Report exact SHA, budget spent,
coverage by plan/model/tier/token class, estimator maturity/bounds, unknowns, and whether the
provider is preview or GA. Never call calibration complete merely because the admin page displays a
number.
