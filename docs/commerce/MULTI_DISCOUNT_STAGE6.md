# Stage 6 — online funding normalization

> **Superseded (2026-08-07).** The commerce firing pins for this stage (admin routes, worker lanes, deploy gates) were removed with the dismantled release cycle — see `docs/ops/MODEL_RELEASE_CYCLE.md`. This file is kept as the protocol record; the engine-side producers under `/admin/pricing/v2/*` are unchanged.

Status: the engine producer, the strict TypeScript transport consumer, and the bounded commerce
orchestration are implemented on top of the already delivered migration
`0029_pricing_release_two_phase_finalize.sql`.
The orchestration binds to the exact Stage 5 plan, computes the funding manifest only from the
actual full-inventory readback, and then finalizes the target/recovery releases in two phases. The
existence of the code by itself starts nothing: the worker begins POST only after explicit
idempotent staging. Stage 6 does not require a maintenance window, stopping the money writers,
zero reservations, or manual account verification, and it activates nothing.

## Source policy

Funding normalization is needed for paid/bonus attribution and referral math, not for restricting
available models.

- an exact non-revoked `signup-bonus:<subject>` is preserved as `welcome_bonus`;
- an exact pair `signup-bonus:<subject>` + full negative
  `bonus-revoke:<subject>` removes the welcome entitlement: the entire current aggregate becomes
  `paid`;
- an existing grant retains its actual nominal value `$4`;
- new grants after the contract change have a nominal value `$5`;
- all other existing balance is classified `paid` by the owner's decision;
- a paid lot is materialized even at zero residual: it is the immutable anchor for the permitted
  `$1` overrun on a bonus-only/zero-hold request;
- bonus is allowed for any model available to the B2C policy;
- reserve spends bonus-first, then paid;
- referral commission receives only the paid-funded settlement amount.

The `track` mode, `track` eligibility, and the `welcome_track_bonus` bucket are not created by the
new code. Immutable historical rows may keep the old values only as audit evidence.

## Preparing the writers

Before the backfill, the production runtime must:

1. save a v2 pricing/funding snapshot in every new reservation;
2. dual-write topup, bonus, reserve, cancel, settlement, and refund to the aggregate and funding
   lots in a single account transaction;
3. take the same account row/advisory lock as the backfill;
4. re-read the funding generation after waiting for the lock;
5. be able to complete an old reservation by its immutable legacy snapshot.

This application rolls out blue-green under the old active release and by itself does not change
the price.

### Pre-cutover writer checkpoint

The PostgreSQL writer selects its path only after account-local serialization:

```text
reserve/settlement: request advisory lock → funding account advisory lock
                    → reread active funding head → row locks/money writes
topup/adjust:       funding account advisory lock → reread active funding head
                    → row locks/money writes
```

A missing head means a fully legacy transaction. An existing head means a mandatory dual-write: the
account aggregate, the active generation, the lots, and the reservation snapshot/allocation either
commit together or fully roll back. The same rule closes the race with normalization: a writer that
waited for its lock re-reads the already new head and cannot continue as a legacy writer.

Reserve saves the bonus-first allocation. Overdraft is permitted only in paid and not beyond the
old account floor `$1`; the normalized generation must contain a paid lot even with zero residual,
and a bonus-only or zero hold keeps it as a zero allocation anchor, so that a possible settlement
overrun is not erroneously attributed to bonus. Cancel returns the entire hold by the saved
allocations. Settlement turns exactly these allocations into charged/released, updates the lots,
and writes the charge attribution to `funding_ledger_allocations_v2`. An exact terminal replay
charges nothing again and checks the original immutable generation even after a subsequent
monotonic head advance.

`account_topup` classifies a positive `signup-bonus:*` as `welcome_bonus`; all other credits and
negative adjustments — as `paid`. An exact idempotency replay returns the first ledger row before
any repeated lot mutation. Durable outbox recovery executes the same settlement path.

Real PostgreSQL evidence —
`pg::tests::pre_cutover_funding_v2_writer_postgres_matrix`: bonus-first/replay/cancel/settlement,
paid overrun, top-up/bonus/adjust, recovery after enqueue, writer after normalization wait, and
verification that settlement does not seize the reservation row before the funding-account lock.

While the global release head is absent, the snapshot is composite: the existing immutable
`pricing_admission_snapshots` pins the old active price, while
`funding_reservation_snapshots_v2` together with `funding_reservation_allocations_v2` pins the
exact funding generation and bonus-first lots. This is necessary because the full prepared release
itself references already normalized funding generations. After Stage 9, new requests atomically
write the release-linked tables of migration 0023; pre-cutover rows continue to complete by their
composite snapshot and are not recalculated.

## Online plan/apply

The planner builds a content-addressed plan over the entire inventory. No manual
resolution/reviewer artifact is used. For each account the plan contains source-state/ledger
digests, exact target lots, and structural blockers.

The engine producer provides only account-local operations under the control key:

```text
GET  /admin/pricing/v2/funding/{account_id}/normalization
POST /admin/pricing/v2/funding/{account_id}/normalization
     {expected_source_state_digest, expected_normalization_digest}
```

`GET` operates in `REPEATABLE READ READ ONLY` and returns `ready|blocked|normalized`, canonical
`sha256:v2` source/target identities, exact lots, and typed blockers. `POST` runs in
`SERIALIZABLE`, first takes the same funding-account advisory lock, then fully rebuilds the plan.
The response `stored|unchanged|stale|blocked|conflict` does not permit applying edited or stale
JSON. SQLite responds fail closed: the live authority of this transition is PostgreSQL only.
`packages/contracts` validates the full strict wire shape and canonical digests, and the only typed
calls live in `packages/engine-client`; the transport consumer by itself does not start the
backfill.

When consistent legacy `funding_buckets` exist, the exact historical `welcome_track_bonus` is
carried into the provider-independent `welcome_bonus`, and all other buckets collapse into `paid`.
If legacy buckets are absent, the planner restores the welcome amount from the immutable
`signup-bonus:*` top-up and `balance_after_nano`; charge rows deleted by retention are accounted
for as exact negative gaps between the surviving money rows. Without welcome evidence the entire
aggregate becomes paid. The same holds after an exact same-subject/full-amount `bonus-revoke:*`:
this is a revocation of the entitlement, not a spend of the welcome lot, so pre-revoke gaps remain
historical evidence and do not create an active bonus. A partial, mismatched, duplicate, or mixed
revoked/active grant remains `invalid_ledger_evidence`. In every variant a zero paid anchor is
created.

Apply proceeds in bounded batches. Each account-local `SERIALIZABLE` transaction:

1. takes the account money lock;
2. re-reads the aggregate, the ledger, reservations, and existing lots;
3. checks the expected source digest;
4. computes the exact unused welcome remainder;
5. attributes the residual balance/reserved/spent to paid;
6. checks the sums and overflow;
7. if active legacy reservations have a single proven paid-only attribution, atomically creates
   immutable funding snapshots/allocations for them on the paid lot; the old pricing snapshot
   remains unchanged;
8. atomically writes the lots, the funding generation, and the head.

Paid-only adoption is permitted only when the active holds exhaust the exact account reserved
aggregate, no welcome lot owns any reserve, and the ledger replay without legacy buckets
additionally proves a fully exhausted welcome remainder. Any live bonus/paid ambiguity remains
`active_legacy_reservation`; post-reserve attribution is not guessed.

Other accounts are not blocked. A request for the current account may briefly wait for its money
lock, after which it executes entirely against the state before or after normalization.

Apply is resumable and idempotent: an exact account replay does not create duplicates. A stale
account is replanned without rolling back already completed accounts. A global partial result is
acceptable during the backfill because the active release remains legacy; Stage 9 requires 100%
readiness.

## Bounded orchestration

The two-phase `stageFundingNormalizationJobV2` accepts only the exact `plan_digest` of a fully
ACKed Stage 5 run in the `materializing` state, whose target/recovery skeletons still have nullable
final identities, and idempotently creates one
`pricing_release_control_jobs_v2` with `job_kind=normalize_funding`. The payload identity binds the
immutable Stage 5 run/plan, both skeleton plan digests, the engine/service inventories, and the
funding-plan digest, but does not contain the not-yet-existing final funding manifest. `planned`,
blocked/failed, already-finalized, or a changed Stage 5 run is rejected before the job is created.
Replaying an already confirmed `prepared` run returns the same job ID. The worker does not look for
a release to launch automatically: the absence of an explicitly staged job means the absence of any
normalization POSTs.

On each resumable slice the worker:

1. recovers only expired parent/account leases;
2. exhausts the engine cursor twice with a bounded page size and rejects a duplicate/regressing
   cursor;
3. compares the stable identity `(account_id,status,multiplier_bp)`. Live
   `balance/reserved/spent` and the funding head are deliberately not part of the coverage digest:
   money writers keep running and serialize with the apply account-local lock;
4. separately verifies the canonical service inventory. All service accounts are excluded from the
   funding queue because their release assignment is `meter_only` without a funding generation;
5. fetches and applies no more than the configured account batch. Before each POST a new GET is
   performed, and the POST receives only the exact digests from that response;
6. performs a final full inventory scan and a repeated coverage check before confirmation.

`active_legacy_reservation` returns only its own account to `retry` when funding attribution
remains ambiguous. A proven paid-only active reserve is normalized without waiting for an idle gap
and without stopping new requests. The remaining typed blockers are saved as `blocker` with the
exact `source`, `source_state_digest`, and `blockers[]`; `conflict` likewise remains fail closed.
`stale` is replanned. `normalized`, `stored`, and `unchanged` are recorded as `ready`. Expired
leases do not lose plan identity, and a bounded retry does not roll back already ready accounts.

The parent becomes `confirmed` only when all of the following conditions hold simultaneously:

- the final engine identity digest and the service inventory digest match the immutable target
  plan;
- the queue contains exactly all balance accounts and not a single service/missing/extra account;
- every row is `ready`, has a positive funding generation, and
  `applied_funding_digest=target_funding_digest` without blockers;
- the canonical funding manifest is computed from the exact ready queue and does not yet conflict
  with an already finalized identity.

A new account after this evidence invalidates the next Stage 8 inventory digest; Stage 9 once again
checks full coverage under the global release lock immediately before the single-head CAS. The
first short `SERIALIZABLE` confirmation transaction fills each balance assignment only with the
`funding_generation: NULL → positive` transition, saves the identical canonical
`funding_manifest_digest`, and copies the exact funding evidence into the pre-prepared recovery
plan. Any replacement, missing/extra assignment, or ready-queue mismatch rolls back the entire
local finalization. After commit the consumer builds the exact target/recovery engine releases and
the recovery link, performs three prepare+readback operations without holding a PostgreSQL
transaction, and then in a second short `SERIALIZABLE` transaction saves the three ACKs, the
`engine_release_digest`, the target/recovery digests, and the `prepared` status. A retryable
transport/DB failure leaves the parent unconfirmed and permits an idempotent retry; immutable
readback drift or a changed local bundle stops fail closed. DB guards forbid an earlier `prepared`
and freeze assignments after it. The runner does not create an activation job/Stage 8 evidence,
does not move the release head, and does not change balances or the pricing policy.

## Staging and status

Production status and staging are available only through the AdminGuard-protected commerce API.
Both endpoints require a verified `x-admin-actor`; stage additionally accepts a meaningful
`reason`. Status writes nothing and shows the lineage, the states of both plans/job,
attempts/last error, the queue breakdown, and both final identities. Stage idempotently creates the
job, atomically writes the attributed audit request, and returns the same strict status snapshot
with `staged_job_id`:

```text
GET  /v1/admin/pricing-stage6-v2?plan_digest=sha256:v2:<exact-stage5-plan-digest>
POST /v1/admin/pricing-stage6-v2/stage
{"plan_digest":"sha256:v2:<exact-stage5-plan-digest>","reason":"normalize reviewed full inventory"}
```

The endpoints do not need engine credentials: the account-local and release prepare work is
performed by the already running worker. The package CLI remains a diagnostic non-production
entrypoint and is not launched over SSH. The existence of code, the API, or local integration
evidence does not substitute for the production `confirmed` status.

The worker's bounded parameters have safe defaults and hard limits:

```text
FUNDING_NORMALIZATION_POLL_MS=5000
FUNDING_NORMALIZATION_BATCH_SIZE=25              # 1..500 account operations per slice
FUNDING_NORMALIZATION_INVENTORY_PAGE_SIZE=500    # 1..500 rows per cursor page
FUNDING_NORMALIZATION_LEASE_MS=300000             # 30s..1h, heartbeat on cursor pages
FUNDING_NORMALIZATION_RETRY_MS=15000              # 1s..1h
```

## In-flight contract

Zero reservations is not required. Legacy-format reservations/outbox rows created before the
dual-compatible runtime continue to complete naturally before or after Stage 9 by their
reserve-time identity. New requests keep arriving and already carry a v2 snapshot; both formats
cross Stage 9 without any price or funding allocation recalculation.

A hot account also does not have to happen to fall into an idle gap: when the welcome amount is
fully exhausted or another exact authority already proves a paid-only reserve, Stage 6, under the
shared account lock, adds only the funding identity to unfinished legacy reservations. Their
immutable pricing identity and the client stream do not change. If even part of the reserve may
belong to welcome, the account waits for its natural terminal state and remains fail-closed.

## Blockers

There is no manual financial review, but the automatic arithmetic remains fail closed. Stage 6
cannot declare an account ready in case of:

- a mismatch between the aggregate and the sum of lots;
- a partial/mismatched/duplicate `bonus-revoke:*` that does not prove the full revocation of every
  retained `signup-bonus:*` of the same subject;
- negative/overflow violating the money invariants;
- a conflicting idempotency reference;
- an unfinished legacy reservation for which there is no honest paid-only attribution, so a funding
  snapshot cannot be atomically created without guessing;
- a change in account state after the expected digest was built.

Such a blocker is fixed by code or by a repeated plan on fresh state; production traffic is not
stopped for it.

The durable queue stores the exact `source_state_digest`, `source`, `blockers[]`, and a nullable
target identity. For `ready`, the target generation/digest and the exact applied digest remain
mandatory. This makes it possible to preserve an honest blocked plan without placeholder values;
the existence of the expand schema by itself does not start the backfill.

## Completion evidence

Stage 6 is complete when the confirmed parent proves the exact full-inventory funding manifest,
every balance account target and the recovery plan has an exact immutable funding generation, the
final manifest is atomically saved, all new writers are dual-compatible, the legacy-format inflight
count is observed, format-aware settlement is proven, and a full replay returns only `unchanged`.
After the engine prepare/readback, the evidence feeds into Stage 8 and both final release
identities. The existence of runner code without a staged/confirmed production job does not count
as completion.
