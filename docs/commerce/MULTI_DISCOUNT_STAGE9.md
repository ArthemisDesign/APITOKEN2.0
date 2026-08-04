# Stage 9 — atomic full-inventory cutover

Stage 9 moves all customers simultaneously, without canary and without stopping production. The only
live mutation is a compare-and-set of a single global active pricing release head.

Commerce migration `0031_pricing_activation_evidence_capture.sql` is a separate expand-only
checkpoint ahead of the consumer code. It adds nullable storage of the source engine evidence
digest/exact capture time, the immutable activation request, and the full validated engine receipt.
The migration does no backfill, creates no activation job, and invokes no engine CAS; the old
runtime keeps working, and the dependent consumer is delivered only after GREEN `deploy/migration`
and `deploy/watchdog` on that schema SHA.

Commerce migration `0032_pricing_activation_service_evidence.sql` is the next expand-only
checkpoint. It adds a nullable service-inventory digest, does no backfill, and creates no job.
After its GREEN SHA, the new collector stores the digest in every evidence row, staging rejects an
old row with `NULL`, and the first delivery requires the fresh service authority to match exactly
this persisted digest. This way a post-cutover service account cannot appear between the evidence
and the recovery CAS unnoticed.

Commerce migration `0033_pricing_stage8_managed_capture.sql` is a separate expand-only checkpoint
for the regular production capture without SSH and file handoff. It creates only an empty queue and
an append-only store of the source engine JSON/combined JSON per attempt. The migration creates no
job, calls no engine, collects no Stage 8, and activates no release. After the GREEN schema SHA, the
engine publishes the protected read-only
`POST /admin/pricing/v2/stage8-evidence/capture`: the strict request contains only exact generations,
frozen window/sample limits, and the Gemini admission aggregate, and the compile-fixed runtime
manifest is attached by the server itself. The endpoint returns a schema-v2 report and, at
`passed=false`, runs the existing `REPEATABLE READ READ ONLY` collector through a bounded PostgreSQL
reader and does not change the head, accounts, balances, reservations, or traffic. This producer
itself creates no job/caller. Only after GREEN on the exact producer SHA does a separate commerce
worker/API consumer read the response as raw text via `json-bigint`, store the source JSON, and only
then is the admin UI connected. None of these steps stops traffic or money writers.

After the GREEN schema SHA, the durable consumer adds strict contracts, the single typed engine
transport, and a worker lifecycle `pending → processing → retry|dead|confirmed`. Explicit staging
stores the immutable request before any network call and accepts only persisted `passed=true`/zero-blocker
evidence with prepared target/recovery engine digests. Before the first network delivery, the worker
in a fresh `SERIALIZABLE` snapshot re-exhausts the engine and OpenKeys inventory, reconciles
commerce/service ownership, status, B2B scalar authority, and the canonical OpenKeys 1:1 prepared
target policy, and after the cutover requires an exact paired
assignment extension, policy, and active funding generation for every new account. Only subject
digests leave the system. Once the exact request has been handed to the transport lane for the first
time, a timeout, crash, or lost ACK repeats only this stored body without a TTL/mutable-authority
preflight: the CAS may already have been applied, so a new interpretation of the retry would be
unsafe. Success atomically stores the complete validated ACK and the canonical request/receipt
result digest.
The recovery expectation is not reconstructed: it is read only from the full durable cutover receipt.
The consumer does not create jobs automatically. The collector now populates the source engine
identity from migration 0031 and the service identity from migration 0032; any legacy evidence row
with `NULL` in these fields is unfit for staging.

Until the first CAS, the OpenKeys source contract and `accounts.mult_bp` remain legacy runtime
scalars: they must not be rewritten to 10000 in advance, because that would detach OpenKeys from the
simultaneous cutover. The preflight proves 1:1 via the immutable target assignment/document/rules;
after the CAS, the runtime reads this release-v2 policy rather than the old scalar.

## Protected control surface

The commerce producer publishes two additive AdminGuard-protected endpoints:

```text
GET  /v1/admin/pricing-release-activation-v2
POST /v1/admin/pricing-release-activation-v2/stage
```

GET performs a bounded read-only repeatable snapshot of prepared releases, Stage 8 freshness/source
completeness, unresolved pricing backlog, activation jobs, and receipts; the live engine head is
read separately and carries its own observation time/availability, so engine unavailability is not
masked as an absent head. POST requires a strict `activation_kind`, canonical
`evidence_digest`, human `reason`, and a verified `x-admin-actor`. The actor is not accepted from
the JSON body.
The new job and audit row are written in a single `SERIALIZABLE` transaction; an exact replay does
not create a second audit/event. An `accepted` response means only durable staging: the CAS is
performed by the already-existing worker lane after fresh first-delivery authority revalidation.
Transient engine/OpenKeys transport unavailability leaves the job in `retry` without consuming its
first-delivery attempt; semantic drift or malformed authority remains terminal. Therefore a brief
outage before the CAS does not turn a safely staged job into an irreversible `dead` blocker.

This is the only production staging surface. Migration, startup, the evidence collector, GET, and
worker polling cannot create a job. The endpoint does not generate evidence and does not perform an
engine network mutation inline. `apps/admin` is connected by a separate consumer commit after the
GREEN producer SHA.

## Preconditions

- the deployed runtime on both blue-green slots supports the target and recovery release schema, and
  every live claim is bound to its exact owner epoch;
- the old incompatible binary is excluded from the rollback floor;
- Stage 5 target/recovery manifests are materialized and have an exact ACK;
- Stage 6 is complete for 100% of the inventory;
- Stage 7 confirms canonical OpenKeys 1:1;
- Stage 8 combined schema-v2 evidence is persisted, unexpired, and `passed=true`; its source engine
  evidence has passed the canonical digest and 120-second age checks, and engine/OpenKeys have been
  exhaustively scanned twice and did not change between the passes;
- the sales v2 runtime/consumer separately confirms commission only from `paid_funded_nano` and the
  exclusion of the welcome bonus; the `sales_contract_digest` in Stage 8 by itself does not prove
  this;
- shadow evaluation covers 100% of supported requests;
- legacy-format reservations/outbox rows are accounted for as an audit count and continue to settle
  by their reserve-time snapshot; the absence of such rows is not a precondition;
- there are no pending/processing/retry/dead pricing control jobs;
- every active/disabled account has exactly one B2C/B2B/OpenKeys/service assignment;
- account creation/activation uses the shared release control-plane lock.

Active v2 and legacy-format reservations may coexist: each format settles by its own immutable
reserve-time identity. Zero reservations or an artificial traffic pause are not a precondition.

## Apply

The protected control-plane passes the exact target/recovery engine release digests, the combined
Stage 8 evidence identity, the source engine capture time/subdigests, the complete expected head,
the operator, and the reason.
Before calling the engine CAS, it requires an exact immutable commerce row with `passed=true`,
checks `valid_until`, and immediately before the first delivery re-reconciles the
commerce/service/OpenKeys and engine authority against target/recovery. This preflight does not
block traffic or money writers: live balances are excluded from the inventory identity and are
checked separately through active funding generation/head/aggregates. The engine
opens a short `SERIALIZABLE` transaction under the release advisory lock, repeats the engine-side
freshness/coverage checks: immutable pair/link and active catalog/switch lineage, base inventory,
funding manifest/parity, exact runtime-floor digest, and the owner-epoch claim of every live
instance. Then it CAS-advances the single head row to the target generation. Evidence/audit/head
either commit together or roll back entirely.

Apply does not update account bindings, balances, reservations, or ledger rows one by one. After the
commit, any account's new reserve reads the target release. A reservation created before the commit
settles by its stored previous snapshot.

An account created after the cutover receives, before a usable key is issued, an append-only
assignment extension bound to the exact current head and its prepared recovery. The original
full-inventory manifest is not rewritten; the exact extension pair is added in a single transaction
under the same control-plane lock.

An exact replay is compared against the durable activation audit and returns `unchanged`, even if
the ACK was lost and the original TTL has since expired. A retry does not re-read mutable authority,
because a lost ACK cannot be distinguished from an already-applied CAS; it sends byte-for-byte the
same durable request.
Stale evidence, inventory drift, an unsupported runtime or
a claim inherited from another owner epoch, an incomplete funding generation, or a CAS mismatch are
rejected before mutation. A rejection does not require shutting down
traffic: the old release keeps being served.

## Recovery

The recovery release is prepared before apply and has the next monotonic generation. On an automatic
post-activation blocker, a forward CAS to the recovery head is performed; this is neither a return
to the old binary nor a deletion of target artifacts.

Recovery accepts only the complete exact target head from the cutover receipt. Accounts created
after the cutover do not rewrite the base manifest: before the forward CAS, the engine requires
their atomic target/recovery assignment extensions and verifies their active funding heads.
Therefore recovery remains a single head write and does not turn into an N-account rollback. Fresh
engine evidence in this state stores the base inventory digest, but counts each new account as
covered only by an exact paired extension; recovery is not bounded by the TTL of the original
cutover evidence.

Recovery triggers include a systemic rise in pricing/admission failures, funding invariant failures,
settlement backlog, and divergence of the active release readback. A single provider outage is
handled by the provider master-switch and does not by itself roll back the pricing release.

## Post-activation evidence

Immediately after the CAS, the exact active release digest, B2C 50%/override test vectors, one B2B,
one OpenKeys 1:1, service with zero balance, welcome/referral attribution, and cross-cutover
settlement are checked. The final exact SHA must receive a green `deploy/watchdog`.

A maintenance window, global drain, canary selection artifact, and manual approval of money
allocations do not exist in this runbook.
