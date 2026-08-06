# Multi-provider discount contract and online cutover

Document status: approved target contract dated 2026-08-02, LIVE since the Stage 9 one-head CAS
on 2026-08-04 (release generation 13; durable cutover receipt in commerce and the engine head
row). The release-v2 authority governs admission and pricing for every account class. What
remains below the line is the removal of the retired progressive machinery (§6) and the final
Definition of Done sweep. This document supersedes the previous progressive-tariff design: the
target system has no active `track` mode, no tier ladder, and no 30-day retention.

## 1. Accepted product decisions

1. Gemini is a full product provider alongside Anthropic and OpenAI.
2. A regular B2C customer gets a global `50%` discount on every model included in the main
   product catalog.
3. B2C can have a separate percentage discount per provider and a more specific discount per model.
4. B2B does not inherit the global B2C discount. Every B2B customer has its own policy. The current
   scalar percentage becomes a provider-rule for the internal provider ID
   `anthropic` at migration; OpenAI/Gemini are not added to an existing B2B automatically.
5. All existing and new OpenKeys are charged `1:1` at the model's official price
   (`discount_bps=0`, `payable_multiplier_bp=10000`).
6. Service accounts have access to all models supported by the runtime, regardless of the product
   B2C/B2B/OpenKeys catalogs. The domain code decides which models to actually use. The emergency
   master-switch and technical provider unavailability still apply.
7. Service-account consumption is fully metered and persisted, but requests neither reserve nor
   debit the customer balance. This uses a dedicated billing mode `meter_only`, not a zero
   multiplier and not an infinite artificial balance.
8. The welcome bonus is kept. New issuance is exactly `$5.000000000`; previously issued `$4`
   bonuses are not retroactively increased. The bonus is usable for any allowed B2C model and
   provider.
9. Referral commissions are kept. Their eligibility is determined by B2C attribution and the
   referral link, not by the pricing mode. Commission accrues only on the actually charged
   paid-funded portion; welcome-bonus spend is not commissioned.
10. The production migration is performed for the entire inventory in a single global switch.
    Canary and per-account customer enablement are forbidden.
11. Production must not be stopped for the pricing/funding cutover. A global drain, maintenance
    mode, stopping money writers, or waiting for all active reservations to reach zero are not
    allowed.
12. Manual financial classification in Stage 6 is not required. Known unused welcome credit is
    preserved as bonus; all other existing balance is, by explicit owner decision, considered paid.
    The amounts still pass the automatic structural invariants.
13. Converting a customer from B2C to B2B disables every B2C pricing policy for that customer at
    once — the global B2C default, its provider/model overrides, the legacy scalar, and any
    progressive remnant. From the conversion on, only the customer's own B2B policy governs model
    admission and price. The conversion and the per-account strict enforcement cutover are one
    durable flow, not two separate operator steps.
14. Every B2B policy save is enforced immediately: it automatically chains the per-account strict
    cutover (first enforcement) or a strict→strict advance (already strict), overwriting all other
    pricing state for the account — the legacy scalar, every B2C rule, and older policy versions.
    "All past price multipliers are declined" covers new charges only: the immutable ledger and
    in-flight reservations keep their reserve-time pinned price, and every new reserve after the
    flip resolves only the newest B2B policy version. Enforcement never waits for the global
    Stage 9 CAS.
15. Converting a B2C customer keeps his remaining welcome-bonus funds spendable under the new B2B
    policy: the bonus is funding, not a pricing policy. Referral commissions stop at conversion
    because a B2B charge has no commission basis.

`fixed discount` in this contract means a static percentage in basis points. A literal fixed
tariff such as "this model always costs $0.01" is not part of the contract.

## 2. Economics by account class

| Class | Price | Access | Balance dependency | Referral |
|---|---|---|---|---|
| B2C | global `5000 bps`, then provider/model override | main product catalog | yes | paid-funded usage |
| B2B | individual provider/model rules | only models explicitly allowed by the policy | yes | no, until a separate B2B contract is approved |
| OpenKeys | `0 bps` discount, strictly 1:1 | all runtime-priceable models (§4) | yes | no |
| Service | customer charge is not computed; official cost is persisted | all runtime-capable models | no (`meter_only`) | no |

Pricing and admission are different decisions. A discount does not enable a model, and a model's
presence in the catalog does not set its price. The service exception applies only to product
gates; the capability manifest, transport security, and master-switch continue to close a
genuinely unavailable provider.

## 3. B2C discount resolution

All percentages are stored as an integer `discount_bps`:

```text
10000 bps = 100.00%
payable_multiplier_bp = 10000 - discount_bps
charged_nano = floor(official_nano * payable_multiplier_bp / 10000)
```

Resolution happens in strict order:

1. exact `(provider_id, canonical_model_id)` model-rule;
2. provider-rule;
3. global B2C default `discount_bps=5000`;
4. no applicable rule at all — fail closed, not a legacy scalar fallback.

Example:

```text
global B2C       = 50%
provider Gemini = 60%
Gemini image     = 55%
```

A regular Gemini model gets a 60% discount, and an image model gets 55%. A model-rule replaces the
provider-rule entirely; percentages do not stack.

The official cost is first computed by `crates/metering` in integer nanoUSD with an immutable
tariff identity. The policy is applied to the finished official cost. Float/JavaScript `number`
for money is forbidden.

## 4. B2B, OpenKeys, and service

### B2B

The policy belongs to a specific B2B owner and contains provider/model rules with the same
model → provider priority. The global B2C policy never applies to B2B.

At backfill, the existing `mult_bp` is converted as follows:

```text
discount_bps = 10000 - mult_bp
scope = provider:anthropic
```

The migration does not grant B2B access to OpenAI or Gemini. After the cutover an operator may
explicitly add their provider/model rules via a full CAS replacement of the policy. The commerce
policy CAS propagates to the live release-v2 authority through an append-only assignment
extension: a strictly newer version of the account's release policy is prepared and pinned for the
exact current head and its paired recovery, and the runtime resolver prefers the extension over the
immutable base assignment. The base manifest is never rewritten.

### B2C→B2B conversion and immediate policy enforcement

Conversion and B2B policy edits are enforced per account, without waiting for the global
Stage 9 CAS:

- Converting a B2C customer to B2B is one durable flow. It provisions the customer's own policy
  (a single `provider:anthropic` rule derived from the negotiated multiplier unless the operator
  supplies a full rule set), disables every B2C pricing input for the account — the global B2C
  default, its provider/model overrides, the legacy scalar, and any progressive remnant — and
  automatically chains the per-account strict cutover. From the strict flip on, model admission
  and price resolve only against the customer's B2B policy; the flip itself never rewrites the
  immutable ledger and never reprices an in-flight reservation.
- Every B2B policy save overwrites all other pricing state for the account. The first
  enforcement performs the strict cutover; every later save performs a strict→strict advance to
  the new immutable policy version. Older versions, the legacy scalar, and B2C rules never apply
  to new charges again. Enforcement is staged atomically with the save and lands with the
  durable delivery (seconds), not with Stage 9. A policy of any shape — mixed provider/model
  rules included — is enforced by this chain; there is no scalar-only fallback path that would
  leave a saved policy unenforced.
- Before the global release head exists, the chain is the per-account strict lane: funding
  normalization, exact active-policy ACK stamps on every active key, and the atomic
  strict+strict+verified binding job. After the head exists, the same operations propagate
  through the append-only assignment extension pinned to the exact current head and its paired
  recovery. In both eras an already started request settles against its reserve-time pinned
  snapshot, and only new reserves see the newest policy.
- Remaining welcome-bonus funds survive the conversion and stay spendable under the B2B policy
  (funding, not pricing). Referral commissions stop at conversion: a B2B charge carries no
  commission basis, so under-paying commission is impossible by construction.
- The chain is idempotent: replaying a conversion or a save reports the already-enforced state
  instead of staging a duplicate, and a failed precondition (blocked funding normalization, an
  unstamped key, engine-side guards) fails the flow loudly with a typed error, never a partial
  silent state.

### OpenKeys

All OpenKeys, including ones previously considered legacy, receive a canonical immutable 1:1
policy. The history of past charges is not rewritten, but after the global cutover every new
reserve uses `payable_multiplier_bp=10000`. The issuance API has no multiplier/discount override.

By explicit owner decision, OpenKeys access follows the runtime within the pricing authority:
every model of an authority provider (`anthropic`, `openai`, `google`) that the engine can price
is sellable at 1:1 without an OpenKeys catalog cutover. Providers outside the pricing authority
(KIMI, GLM) are not sellable through OpenKeys until a separate authority-extension decision.
The master switch and an explicitly disabled scoped provider switch still close a provider; a
model without a runtime tariff fails closed at quote time.

### Service

The service policy contains purpose/responsible metadata and `billing_mode=meter_only`; it does not
contain a product discount as a way to bypass the balance gate. Every completed request persists:

- account/key/request identity;
- provider and canonical model;
- tariff identity and official cost components;
- actual upstream usage;
- runtime/release lineage.

Customer debit, balance reserve, and `402 insufficient balance` are not performed for service. A
usage/settlement write failure remains fail-closed under the normal durable outbox contract: "does
not depend on the balance" does not mean "accounting may be lost".

## 5. Welcome bonus and funding

A new eligible Google/GitHub B2C registration receives an idempotent credit:

```text
amount_nano = 5000000000
ref = signup-bonus:<commercial-user-id>
source_type = welcome_bonus
eligibility = any_b2c_model
```

Password, B2B, OpenKeys, and service accounts do not receive the bonus. A price change does not
change the face value of an already credited bonus.

Funding lots are needed for honest paid/bonus attribution but do not restrict the bonus to a
specific pricing mode. Reserve spends the welcome bonus first, then paid; settlement and refund use
the exact allocation saved at reserve. Therefore referral receives only the paid-funded portion.

For an existing account the online backfill runs under the same account row/advisory lock as the
money writers:

1. read the current aggregate balance, ledger, and already normalized lots;
2. reconstruct the remainder of exact `signup-bonus:*` credits;
3. record it as `welcome_bonus`;
4. classify all other remainder as `paid` and materialize a zero paid anchor if the residual is
   zero;
5. verify `sum(bucket balance/reserved/spent) == account balance/reserved/spent`;
6. atomically mark the funding generation ready.

There is no manual reviewer artifact and no manual per-account analysis. Arithmetic that does not
reconcile, negative overflow, a replay conflict, or an unknown unfinished legacy reservation is a
technical blocker, not a reason to guess a value.

## 6. Removal of the progressive model

The target runtime, API, UI, and new durable records do not contain:

- pricing mode `track`;
- tier ladder and Starter/subsequent tiers;
- 30-day retention spend/retention eligibility;
- track eligibility and track-only funding;
- commission eligibility's dependency on pricing mode;
- background tier reconciliation/month-close jobs;
- public promises of a progressive discount.

Existing append-only migrations and immutable historical ledger/snapshot rows are not rewritten:
they may contain old rows for audit. New code does not create them and does not use them for
current admission, pricing, funding, or commissions. This is history preservation, not a
compatibility path.

Physical removal of old mutable commerce columns/tables is possible only as a separate late change
after the proven absence of readers/writers. The product semantics are removed before that and do
not wait for schema cleanup.

## 7. Zero-downtime rollout

### 7.1. Why accounts cannot be switched one by one

Sequential migration of bindings creates a mixed production state and requires canary/manual
accounting. Instead, an immutable pricing release is introduced:

- the release contains exact capability/catalog/switch identities;
- the global B2C policy identity;
- all B2B, OpenKeys, and service assignments;
- the funding generation;
- the minimum runtime capability;
- a canonical digest of the entire manifest.

The engine stores prepared releases and one active release head. Preparing a release does not
change traffic. All requests read the active head and the associated immutable data in a single
PostgreSQL snapshot.

### 7.2. Expand and dual-compatible runtime

First, only the new structures are added in separate migration-first commits. Then a runtime is
rolled out blue-green that:

- engine migration `crates/registry/migrations_pg/0023_pricing_release_funding_v2.sql` adds the
  release/funding authority, request snapshots, deferred aggregate/allocation invariants, and a
  nullable v2 lineage of old writer surfaces;
- engine migration `crates/registry/migrations_pg/0024_pre_cutover_funding_snapshots_v2.sql`
  adds an immutable funding snapshot for transitional-period requests, independent of the prepared
  release. It breaks the cycle "release assignment requires a funding generation, while the
  normalized writer requires an allocation snapshot" and does not create a release head, policy, or
  new pricing path;
- engine migration `crates/registry/migrations_pg/0025_pricing_release_runtime_epoch_fence.sql`
  adds a nullable owner-epoch identity for the release-v2 claim. Before the first head, old binaries
  remain compatible; after the head, every claim/heartbeat must confirm the v2 runtime and bind
  that confirmation to a fresh owner epoch rather than inheriting it from the previous process;
- engine migration `crates/registry/migrations_pg/0026_pricing_release_zero_drain_extensions.sql`
  keeps legacy inflight observable but removes the DB traffic-drain requirement for passed Stage 8
  evidence, and adds an empty append-only authority for post-cutover assignment extensions;
- commerce migration `packages/db/migrations/0026_pricing_release_expand.sql` adds the policy,
  inventory, target/recovery plan, resumable Stage 6/control job, Stage 8 evidence, and activation
  receipt authority;
- commerce migrations `0027_funding_normalization_blockers.sql` and
  `0028_pricing_stage5_evidence.sql` add honest nullable blocker plans and immutable Stage 5
  inventory/prepare evidence; `0029_pricing_release_two_phase_finalize.sql` breaks the cycle between
  the live funding state and the release identity: the source/policy/assignment plan is created
  first, and the funding and engine release identities are finalized later under DB guards;
  commerce migration `0030_pricing_stage8_zero_drain.sql` compatibly allows passed combined
  evidence with a nonzero observable legacy inflight count;
  `0031_pricing_activation_evidence_capture.sql` is dormant and adds nullable storage of the exact
  source engine evidence, the immutable activation request, and the full validated receipt for safe
  replay after a lost ACK; `0032_pricing_activation_service_evidence.sql` separately adds a
  nullable service inventory digest so the new collector binds recovery evidence and
  first-delivery revalidation to one exact service authority without backfilling old rows;
  `0033_pricing_stage8_managed_capture.sql` creates an empty durable queue and append-only
  raw/combined artifacts for the protected Stage 8 workflow without SSH/file handoff, but itself
  creates no job, does not call the engine, and does not move the release head;
  `0035_pricing_shadow_rollout_jobs.sql` adds empty parent/child tables for generation-3 shadow
  alignment of the OpenKeys inventory. The parent binds exact prepared target/recovery releases to
  catalog/switch/inventory manifests; the child stores the immutable policy/binding/CAS request and
  the terminal ACK of each OpenKeys account. Commerce/service lineages are aligned by their managed
  policy writers, not by this lane: the engine does not accept a different policy identity on an
  existing lineage, and service `meter_only` is not expressible in a v1 shadow policy. This
  migration creates no rollout/job, activates no legacy policy, and does not change the release
  head. After a GREEN exact engine producer SHA, a separate consumer checkpoint connects the full
  durable lane: strict contracts and typed client (including locked-openkeys-transition),
  `packages/db` staging/lifecycle store, bounded `apps/worker` delivery, and AdminGuard
  staging/read endpoints in `apps/api` (`docs/commerce/MULTI_DISCOUNT_STAGE7.md`). The next
  schema-following producer-first checkpoint added the protected read-only
  `POST /admin/pricing/v2/stage8-evidence/capture`: the server attaches the compile-fixed manifest,
  and the bounded PostgreSQL reader returns the schema-v2 report even when `passed=false`. After a
  GREEN exact producer SHA, a separate commerce checkpoint connects strict contracts/raw-text
  client, explicit AdminGuard staging, durable worker, and bounded status reader. The worker
  persists the exact raw engine bytes before the combined scan, then atomically completes the
  append-only combined artifact and the `passed|blocked` job; no capture path creates an activation
  job;
- sales migration `packages/sales-db/migrations/0015_paid_funded_commission_v2.sql` adds separate
  immutable usage/commission v2 tables without a pricing-mode field.

The compile-fixed pricing runtime manifest accepts frozen capability generations 3 and 4, admitted
generation 5, and dormant generation 6. Generation 4 historically added
`gemini-3-flash-preview`, but its old public-wire live gate returned 404 without usage: the digest is
kept for reproducibility, while the catalog, policy, and release of generation 4 must not be
materialized or activated. After a full fresh Pro+Ultra gate, generation 5 repeats the exact
reviewed Anthropic/OpenAI/Gemini model set under a new digest. Generation 6 adds only
`openai/gpt-image-2-2026-04-21` after real generation and one-reference edit both produced distinct
PNG output with terminal authoritative usage through the existing sealed Codex OAuth pool. It adds
no reseller, image API key, fallback, or public discovery by itself. The Stage 5 materializer uses
the capability, main/OpenKeys catalogs, and switches of generation 6; persisted policy digest
collisions allocate the next immutable policy version. Preparation creates new dormant identities
but does not move the release head or admit customer traffic. The internal provider ID of Gemini in
the pricing authority is `google`; product documents continue to call the provider Gemini. The
OpenKeys generation-6 release catalog keeps the explicit generation-5 Anthropic/OpenAI set and adds
only GPT Image 2.

All three migration surfaces are empty and dormant: the presence of the tables creates no policy,
release head, funding generation, or live consumer. The dependent producer/runtime is allowed only
after green production migration/watchdog on the exact schema SHA.

The first dependent engine producer added PostgreSQL-only `/admin/pricing/v2/*` prepare/read:
immutable policy/release/recovery link, full cursor inventory, and a nullable release head. Later
separate checkpoints delivered v2 runtime claims, release-v2 reserve/settlement, and post-cutover
assignment extensions without creating a head. The current producer-first checkpoint adds the
single `POST /admin/pricing/v2/activate`: a short evidence-gated `SERIALIZABLE` CAS verifies the
exact target/recovery lineage, inventory/funding/runtime subdigests, and owner epochs, then
atomically writes the evidence, audit, and one head row. This SHA has no
contracts/client/application consumer, so a single deploy route does not activate the data-plane.
Typed transport and the durable commerce job are connected only after a green `deploy/watchdog` on
the exact producer SHA.

A separate pre-cutover writer checkpoint connects migration 0024 only after its green production
SHA. Its behavior is intentionally account-local:

- reserve takes the request lock, then the funding-account lock, re-reads the head, and only then
  locks/modifies money rows;
- while the head is absent, the legacy aggregate remains the only writer path; after the head
  appears, the aggregate, active generation, bonus-first lots, and immutable allocation change in
  one transaction;
- top-up/bonus/negative adjust use the same funding-account lock and dual-write after the head;
- cancel/settlement complete the reserve-time allocations saved earlier, and the charge ledger
  receives exact `funding_ledger_allocations_v2` sufficient for a future `paid_funded_nano`
  consumer;
- terminal replay only verifies the snapshot and does not repeat the money mutation; a monotonic
  head transition to the next generation does not invalidate an already terminal snapshot;
- paid overrun is limited by the existing `$1` account floor and may land only in the last paid
  allocation. For a bonus-only/zero hold, the normalized generation contains a zero paid lot, and
  reserve saves it in advance as a zero allocation anchor.

The checkpoint does not create a release head, does not change the active price, and does not yet
enable the release-v2 or `meter_only` data-plane. Those consumer stages remain separate
producer-first releases.

Before launching full-inventory normalization, subsequent producer-first checkpoints must bring
the runtime to the full state that:

- continues serving the current active legacy release;
- can read the new release schema;
- persists an immutable pricing/funding snapshot in every new reservation;
- dual-writes new topup/bonus/reserve/settlement data to the aggregate and the new funding lots;
- supports `meter_only` service settlement;
- does not create new tier/track records.

The new release remains dormant. Therefore the runtime deploy by itself changes neither the price
nor access.

The release-v2 runtime checkpoint connects this dormant schema to all three provider billing paths
but does not add an activation writer. Before the head, admission stays on the current
scalar/bridge/strict format. After the head, a new reserve atomically pins the exact
release/policy/rule/tariff and funding allocations; old request IDs first replay in their original
format. Settlement determines the format solely from the reserve-time snapshot, so legacy and
release-v2 outbox rows can complete simultaneously without a drain. Release-v2 balance settlement
uses the pinned funding generation through the global release-head cutover; an account funding
head cannot be advanced over unfinished allocations. After the monotonic funding-head advance, an
exact terminal replay without a repeated money write is allowed. Service `meter_only` persists the
full official usage with a zero customer debit. The provider adapter remains the authority for the
customer debit: this matters for Codex, where the upstream may report output beyond the requested
cap, while the customer pays for the capped output.

Until the global release head appears, the exact pricing identity stays in the existing immutable
`pricing_admission_snapshots`, and the funding generation/bonus-first allocations stay in the
separate `funding_reservation_{snapshots,allocations}_v2`. After release activation, new requests
use the linked `pricing_request_{snapshots,funding_allocations}_v2`. Both formats pin the
reserve-time decision and let an old request finish after the cutover; one request cannot have both
funding snapshots at the same time.

### 7.3. Online backfill without stopping writers

The backfill proceeds account by account in short transactions. It takes only an account-local
lock. A request of that account may briefly wait for its transaction; the other accounts keep
working.

Stage 5 does not precompute the funding manifest from the moving `balance/reserved/spent/lots`. It
persists immutable target/recovery generations and a full assignment skeleton with nullable balance
funding. After exact full-inventory normalization, Stage 6 fixes every assignment only
`NULL → positive` and the canonical funding manifest in one `SERIALIZABLE` confirmation
transaction. Then the engine prepare/readback creates the final target/recovery digests; only this
state may become `prepared`. Replacing finalized identities and mutating assignments after
`prepared` are forbidden in commerce PostgreSQL, and the release head stays unchanged the whole
time.

New writers use the same lock and re-read the funding generation after waiting. Therefore they
either go fully down the legacy+dual-write path before the backfill, or fully down the new path
after it; there is no lost intermediate write.

Legacy-format reservations and outbox rows naturally complete on the running system, including
after the head CAS, via their saved format-aware identity. New reservations already carry the
release/funding snapshot. Stage 8 accounts for both legacy counts but does not wait for them to
reach zero and does not stop new traffic.

### 7.4. Provisioning race protection

Before the first cutover, the final activation transaction under the control-plane advisory lock
re-inventories all active accounts and refuses to switch the head if even one account is not
classified. While the release is being prepared, a new account must land in both immutable
target/recovery manifests before a usable key is issued.

While the global head is absent, a new account keeps living on the authoritative legacy scalar, and
the prepared versioned policy is confirmed only in `shadow` with `legacy_single` funding.
`reconciliation_state=verified` by itself does not promote the policy to `strict`: strict policy
and strict funding are enabled only by one atomic release-v2 binding. This prevents provisioning
from creating the invalid pair `strict + legacy_single` and getting stuck in the terminal delivery
job before the cutover.

After the global head appears, the immutable manifests are not appended to. Post-cutover
provisioning first creates the account without a usable key, prepares its funding generation (for
balance classes), then atomically adds an append-only assignment extension for the exact current
active/recovery releases. The extension writer takes the pricing release advisory lock, verifies
the exact head, the policy, the active funding head under the account funding lock, and the
account's absence from the base manifests; an exact replay returns `unchanged`. If the release head
changed in the meantime, the writer returns a typed `stale`, provisioning re-reads the new
active/recovery pair and repeats the step without a partial write. Only after an exact GET-readback
is key issuance/activation allowed.

After the green engine producer SHA, a separate consumer checkpoint is connected:
`packages/contracts` verifies the strict body/readback and the identical
account/policy/funding semantics of the active/recovery pair, `packages/engine-client` provides
typed prepare/GET, and `packages/db/src/pricing-provisioning-v2.ts` runs the account-local
orchestration. At `head=null` it keeps the pre-cutover path. After the head appears, it accepts
only a base assignment or performs funding normalization, prepares the exact release-v2 policy,
writes the extension, and reconciles the full GET-readback. `apps/api` performs this check before
remote issue and again before returning the raw key; a postflight error disables the just-issued
key. The consumer's presence does not activate the release and does not create the account in
advance: until Stage 9 it is dormant.

Data-plane reserve/settlement does not take this global lock. During the CAS, only account
creation/activation may wait for milliseconds, but never customer traffic.

### 7.5. Full shadow instead of canary

Before the cutover, the new resolver is computed in shadow for 100% of supported Anthropic,
OpenAI, and Gemini requests. The durable pricing provider ID for Gemini is `google`. Shadow
neither reserves nor debits a second time: the actual snapshot is persisted atomically with the
single legacy reserve, and evaluation compares admission, official cost, resolved discount, funding
availability, and release lineage against the expected target.

No canary-account list is created. Stage 8 accepts only full inventory coverage and the absence of
unexplained discrepancies. A shadow rejection is evidence-consistent only when the balance-billed
target policy has no applicable rule for that provider/model — the same request would fail closed
after the cutover (for example B2B non-Anthropic or OpenKeys Google traffic, which the target
deliberately does not admit). Service `meter_only` evaluations prove resolution and lineage but
skip the target price comparison, since they carry no customer charge. External Gemini usage/admission
counters are audit context only; without
real Google actual snapshots and shadow evaluations, provider coverage is not considered complete.
The mere presence of a Google legacy shadow producer does not enable strict Gemini or the Stage 9
runtime.

### 7.6. Atomic mass cutover

Stage 9 executes one short `SERIALIZABLE` transaction under the pricing-release advisory lock:

1. re-reads the exact Stage 8 evidence and the prepared release digest; before the first network
   delivery the consumer again performs two full engine/OpenKeys scans around the commerce/service
   snapshot, checking ownership/status, B2B/OpenKeys authority, and post-cutover
   extensions/funding;
2. verifies the minimum runtime capability on both blue-green slots and the rollback floor;
3. verifies all active accounts/funding generations and format-aware settlement readiness for the
   observable legacy inflight;
4. verifies the absence of pending/retry/dead control jobs and policy ACK drift;
5. CAS-switches the single active release head to the target generation;
6. records the operator/reason/time and evidence digests in the activation record.

The transaction does not change balances and does not update N account rows. After the commit, the
next reserve of any customer sees the new release. An already started request finishes against the
immutable snapshot saved at its reserve. Therefore traffic is not stopped and one request does not
mix the old price with the new one.

The activation request is persisted before the network. The first delivery requires a fresh TTL and
a mutable-authority preflight; after it, a timeout/crash is treated as a possible lost ACK, so a
retry does not build a new request and does not repeat the mutable preflight — it sends the exact
durable body. An applied CAS then idempotently returns `unchanged`. No blocker publishes raw
account/owner identity.

### 7.7. Recovery without downtime

Before the cutover, a recovery release of the next monotonic generation is prepared with
v2-compatible semantics of the previous production price. If post-activation automation detects a
systemic error, it activates the recovery generation via the same single-head CAS. Rolling back to
an old binary that does not understand the new release/funding schema is forbidden.

Rollback does not delete immutable policies, funding lots, or snapshots and does not revert
completed charges. An exact replay of activation returns `unchanged`.

## 8. Re-scoped stages

### Stage 5 — target materialization

The planner builds B2C 50%, B2B Anthropic migration, canonical OpenKeys 1:1, and service
`meter_only` assignments. A B2B owner whose live policy head was already CAS-extended with extra
provider/model rules receives a target policy that mirrors the current head exactly, so the cutover
never reprices or closes already granted live traffic; the head must keep the `provider:anthropic`
rule equal to the live scalar, and an inexpressible legacy rule is a typed blocker. There is no
separate manual assignment matrix: the authoritative
inventories must fully and unambiguously cover the active engine accounts; any collision/missing
owner blocks apply.

### Stage 6 — online funding normalization

Stage 6 becomes a resumable online backfill with the owner-approved rule "exact remainder welcome,
everything else paid". It requires no maintenance window, reviewer artifact, or zero reservations.

### Stage 7 — OpenKeys

OpenKeys issuance moves to the canonical 1:1 policy in advance. The existing inventory is also
prepared for the 1:1 target release; the live price changes only together with all accounts at
Stage 9. Replacement-locked legacy bindings are not bypassed via generic prepare/activate and are
not rewritten account-by-account in multiple calls. A dedicated engine producer
`POST /admin/pricing/policy/{account_id}/locked-openkeys-transition` atomically inserts only the
exact next managed provider-only 1:1 successor and CAS-switches the exact legacy binding to
`shadow + legacy_single + verified`. The catalog/switch target must already be active, the identity
is preserved, both versions increase by exactly one, an exact replay returns `unchanged`, and
discount/model/track/retention/commission rules are forbidden. This pre-Stage-8 step does not
change the live price, funding authority, or global release head; the protected commerce rollout is
connected after a GREEN producer SHA as a durable shadow-rollout lane (migration 0035): the
AdminGuard staging endpoint, the `packages/db` store, and the bounded `apps/worker` consumer cover
the entire exact Stage 5 inventory, including OpenKeys and service. The protocol is
`docs/commerce/MULTI_DISCOUNT_STAGE7.md`.

### Stage 8 — full-inventory evidence

Read-only engine evidence and immutable combined commerce evidence bind the exact
commerce/engine/OpenKeys/service inventories, all policy ACKs, funding generations, 100% shadow,
runtime capability, and the prepared/recovery release digests. The engine report v2
accepts exact target/recovery generations and in one snapshot hashes the current engine inventory,
the target funding manifest, the shadow evaluation set, and the live runtime floor. It requires
identical funding/runtime lineage of target/recovery, full assignment coverage of active and
disabled accounts, parity of funding heads/lots with the aggregate, each shadow result matching the
exact target rule, and observable legacy-format inflight counts without a traffic-drain
requirement. The commerce consumer starts only from an explicitly staged immutable job with a UUID
idempotency key, actor/reason, and exact capture bounds. It receives the engine artifact through a
single typed transport, persists the original raw bytes before any dependent reads, verifies the
canonical Rust digest without i64 loss, scans OpenKeys twice, re-reads the current commerce/service
identities and semantic target/recovery assignments, then persists the schema-v2 identity for 300
seconds and completes the job/artifact in one transaction. Legacy-format inflight reservation/outbox
remains only an audit count and is no longer added to engine blockers; the format-aware snapshot
remains mandatory. Blocked evidence is persisted with `passed=false` and terminal job status
`blocked`, and the absence of a local release pair does not create a row. Uncertain failures use a
bounded retry/lease/attempt state machine; a protocol conflict and the last attempt fail closed
into `dead`. The global claim fence does not allow two simultaneous captures. Startup, migration,
polling, and the activation request do not stage a capture, and capture completion does not stage
activation and does not move the global head.
The release-v2 assignment/funding authority is the only cutover proof: a legacy shadow binding may
remain `reconciliation_state=pending`, and the `funding_buckets` table empty after Stage 6 does not
create a second blocker. For OpenKeys, preflight verifies the canonical 1:1 policy in the prepared
target release; the current source/engine scalars are not changed in advance and stop being the
pricing authority only together with all customers in the single Stage 9 CAS.
After the exact target CAS, engine Stage 8 switches the inventory check to the immutable base plus
the exact paired assignment extensions and their live funding parity. Therefore a new account does
not make fresh recovery evidence unreachable after the original TTL expires.
Stage 9 accepts only a fresh persisted `passed=true` identity and re-verifies the authority.
`sales_contract_digest` pins the expected `paid_funded_nano` contract, but the deployed sales
runtime is proven by a separate checkpoint. The producer intentionally stays blocked until all live
engine instances publish release/funding schema v2 claims; this claim writer is delivered by the
Stage 9 runtime checkpoint, not bypassed by weakening Stage 8.

### Stage 9 — one-head activation

The canary planner is removed. The only apply action is a CAS of the active release head for the
entire production inventory. The engine producer already implements cutover from an absent head,
exact lost-ACK replay, and forward recovery from a complete target head. The durable commerce
consumer is connected after a GREEN schema SHA: the strict request is persisted before the network,
a lost ACK repeats the exact body, a complete ACK and the canonical request/receipt digest are
committed before `confirmed`, and the recovery expectation is taken only from the cutover receipt.
It does not activate itself: migration, startup, Stage 8, and the read-only API create no job. The
only producer is an intentional authenticated Admin API staging call with the canonical evidence
digest, verified actor, and reason; the read endpoint stages nothing by itself. Source engine and
service identities are already persisted by the new collector checkpoint, and legacy `NULL` rows
fail closed. Stage 9 does not stop the service and requires no manual financial sign-off.

## 9. UI and API contracts

The customer B2C pricing view shows the effective discount per provider/model and which rule
level produced it — the global default, a provider override, or an exact model override. It does
not show the tier, retention progress, or internal release digests. B2B sees only its own
policy. OpenKeys shows 1:1. Service usage is available to operators as official cost without a
balance.

The admin pricing editor must:

- manage provider and model B2C rules; the global B2C default and the service `meter_only`
  policy are pinned by the active release and change only through a new release cycle
  (prepared target/recovery pair, fresh Stage 8 evidence, one head CAS), never through a direct
  editor save — the editor refuses such a save with `release_cycle_required`;
- show the effective preview and the exact-model priority;
- not offer `track`/tier controls;
- not apply B2C rules to B2B/OpenKeys/service;
- manage the B2B full policy; post-cutover a B2B policy save and a B2C→B2B conversion propagate
  to the live authority through an append-only assignment extension (the pre-cutover automatic
  strict chain has stood down) and surface the enforcement state;
- show service as all-model `meter_only`;
- show the prepared/active/recovery release and Stage 8 freshness.

The sales feed, after the expand-only producer-first transition, receives `commission_eligible`
regardless of pricing mode and the exact `paid_funded_nano`. The old fields may temporarily remain
for the deployed consumer, but the new consumer does not use them; removal is the last contract
step.

## 10. Mandatory tests

- B2C: global 50%, provider override, model override, model-over-provider precedence.
- B2B: scalar migration only into `anthropic`; no B2C inheritance.
- Conversion: B2C→B2B disables the global B2C default, overrides, and the legacy scalar for the
  account (pre-cutover via the automatic strict chain, post-cutover via the class-changing
  assignment extension), and only the B2B policy prices new reserves;
  remaining welcome bonus stays spendable; the charge carries no commission basis.
- B2B policy save: first enforcement cuts over, later saves advance strict→strict; a mixed
  provider/model policy is enforced without a scalar bridge; in-flight settlement keeps the
  reserve-time pinned snapshot; older policy versions never resolve again; replay is idempotent.
- OpenKeys: existing/new 1:1; any discount override is rejected.
- Service: all runtime models are available; a zero balance does not produce a 402; official usage
  is durable.
- Welcome: new issuance of $5, exact idempotency, old $4 not increased, all B2C providers allowed,
  bonus-funded usage is not commissioned.
- Referral: paid-funded commission is preserved without pricing-mode eligibility.
- Funding backfill: concurrent topup/reserve/settle, account-local lock, exact replay, bucket sums.
- Pre-cutover writers: real PostgreSQL bonus-first reserve, terminal replay after a funding
  generation advance, cancel/refund, paid overrun, top-up classification, outbox recovery, and a
  proven lock order with no reservation-row lock before the funding-account lock.
- In-flight cutover: a reserve before activation and a settlement after it use one snapshot.
- Provisioning race: a pre-cutover account is covered by the target/recovery manifests; a
  post-cutover account gets an atomic exact-head active/recovery extension before a usable key;
  replay/stale/conflict and resolver readback are proven on real PostgreSQL.
- Stage 8: 100% inventory/shadow, Gemini, format-aware legacy inflight audit, exact runtime floor.
- Stage 9: single-head atomicity, no N-account writes, exact replay, stale evidence rejection.
- Recovery: forward activation of the recovery generation without an old binary and without a
  traffic stop.
- Cleanup: active code/API/UI neither creates nor reads tier/retention/track semantics.

## 11. Definition of Done

The work is complete only when all of the following hold simultaneously. Production state as
verified on 2026-08-06 (read-only audit of commerce/engine PostgreSQL and the GitHub deploy
statuses): items 1, 3–13, 15 hold — the cutover receipt and the generation-13 head are
durable, funding normalization is `ready` for the full inventory, Stage 8 evidence passed, the
B2C global rule resolves at 5000 bps, B2B policies carry their negotiated provider rules,
OpenKeys bill 1:1 with runtime-following admission (Google included), service accounts run
`meter_only`, new signups receive exactly `$5.000000000`, and every deploy lane is green.
Item 16 was closed on 2026-08-06 by the settlement floor fix (`fix(pricing): settle release-v2
charges with the exact contract floor`): every release-v2 settlement path now floors like the
reserve, and the Stage 8 shadow validates the same formula. Items 2 and 14 are closed by the
progressive cleanup and the residual-track UI removal: no active writer creates
tier/track/retention records and no UI/API surface presents them; only immutable history and
the legacy columns remain, pending the late physical schema cleanup. Item 17 is closed by the
class-changing assignment-extension lane (engine migration 0034 + the commerce consumer in
`fix(pricing): propagate post-cutover B2C-to-B2B conversions into the release authority`): a
post-cutover conversion now propagates through the exact active/recovery extension, key
issuance self-heals through the same path, and every other class mismatch still fails closed.

1. All expand migrations are delivered before the dependent code.
2. No new writer creates progressive pricing records.
3. All active accounts are unambiguously classified as B2C/B2B/OpenKeys/service.
4. Gemini is present in the main catalog; product-specific OpenKeys enablement is set explicitly.
5. B2C global/provider/model resolution matches the contract.
6. B2B is migrated only into the Anthropic rule; the OpenKeys target is strictly 1:1.
7. Service works with a zero balance and fully accounts for official usage.
8. The new welcome bonus equals $5; the referral paid-funded math is green.
9. The online funding backfill is finished without a global stop.
10. Stage 8 is green on the full inventory and 100% shadow.
11. The prepared target and recovery releases are exact and supported by the current runtime floor.
12. Stage 9 switched the entire production inventory in one CAS; no canary was used.
13. Post-activation smoke and monetary invariants are green on the exact deployed SHA.
14. Public/admin/customer/sales documents and UI no longer promise tiers or a track-only bonus.
15. `deploy/watchdog` is green on the final SHA.
16. B2C enforcement is audited against the global 50% default plus provider/model overrides; any
    deviation that bills a B2C customer off-policy is fixed.
17. Conversion and every B2B policy save enforce the customer's own B2B policy per account.
    Pre-cutover this ran through the automatic strict chain; post-cutover it propagates through
    the append-only assignment extension (a class-changing extension for conversions). No B2C
    rule or legacy scalar prices a converted account's new charges, and past multipliers survive
    only in immutable history.

Production mutation must not be performed from a research or documentation task. Applying Stage
6/8/9 happens only after the implementation, tests, and standard delivery of each expand-only
producer step.
