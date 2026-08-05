# Multi-discount implementation audit — 2026-08-05

**Subject.** Implementation of the approved target contract in
[`docs/commerce/MULTI-DISCOUNT.md`](../commerce/MULTI-DISCOUNT.md), including the engine pricing
runtime, Commerce control plane and workers, customer/admin surfaces, OpenKeys, Sales attribution,
migrations, tests, and production-rollout evidence.

**Audited snapshot.** `origin/master @ 7b000805a961e7b7aa54f38c4c2bd268bfdc11fa`.

**Method.** The audit was performed in an isolated read-only worktree. Eight parallel scoped reviews
covered engine/registry, Commerce DB/workers, API/admin, OpenKeys/web, Sales, migrations/rollout,
tests/cleanup, and architecture/economics. High-risk conclusions were rechecked directly against the
source. Focused Rust tests were run; production pricing state was not queried or mutated.

---

## Executive verdict

The release-v2 pricing architecture and most Stage 5–9 control-plane machinery are implemented and
generally defensively designed. The difficult foundations are present: immutable policies and releases,
integer money, exact rule precedence, reserve-time lineage, account-local funding normalization,
full-inventory evidence, durable lost-ACK replay, one-head activation, and forward recovery.

The implementation is nevertheless **not ready to be declared complete against the target contract**.
The principal blockers are:

1. active worker, API, admin, and customer code still creates, reads, and displays progressive
   `track`/tier/retention semantics;
2. final release-v2 settlement uses half-up rounding while the contract requires floor;
3. customer and global/service admin surfaces remain connected to the legacy policy authority and
   cannot represent the target global rule/source contract;
4. the canonical documents contain incompatible OpenKeys admission decisions, while current runtime
   provider coverage is also wider than release-v2/Stage 8 coverage;
5. the Sales v2 consumer silently accepts conflicting replays of supposedly immutable evidence;
6. repository and deployment health do not prove that production Stages 5–9 and the full Definition of
   Done were executed.

The correct status is therefore:

- **release-v2 foundation:** substantially implemented;
- **target-contract code readiness:** not complete;
- **production cutover completion:** unverified by this audit.

---

## What is implemented

### Release-v2 economics and authority

- Immutable policy, rule, assignment, release, request-snapshot, and evidence identities are defined in
  `crates/registry/src/pricing/release_v2.rs`.
- B2C validation requires the global `5000 bps` rule; B2B is independent; OpenKeys policies are 1:1;
  service policies are catalog-free, rule-free `meter_only`.
- The PostgreSQL resolver evaluates exact model, then provider, then global rule, and fails closed if no
  rule applies (`crates/registry/src/pricing/postgres.rs:4561`).
- Assignment extensions supersede immutable base assignments without rewriting release manifests.
- Anthropic, OpenAI/Codex, and Gemini billing paths consume release-v2 resolution and preserve exact
  reserve-time lineage.
- `meter_only` persists official usage while preventing balance reservation/debit.

### Zero-downtime rollout machinery

- **Stage 5:** deterministic full-inventory classification and materialization of B2C, B2B, OpenKeys,
  service, target, and recovery authority (`packages/db/src/pricing-stage5-materializer-v2.ts`).
- **Stage 6:** resumable account-local funding normalization under the same lock order as money writers
  (`packages/db/src/funding-normalization-jobs.ts`,
  `crates/registry/src/funding_normalization_v2.rs`).
- **Stage 7:** durable OpenKeys shadow rollout, including the dedicated replacement-locked transition
  (`packages/db/src/pricing-shadow-rollout-jobs-v2.ts`).
- **Stage 8:** read-only engine evidence, raw-first persistence, integer-safe Commerce combination,
  double OpenKeys scans, funding parity, shadow/runtime floor, and durable passed/blocked artifacts
  (`crates/registry/src/stage8.rs`, `packages/db/src/multi-discount-stage8-evidence.ts`).
- **Stage 9:** explicit authenticated staging, request-before-network persistence, exact lost-ACK replay,
  one global release-head CAS, and forward recovery
  (`packages/db/src/pricing-release-activation-jobs.ts`,
  `crates/registry/src/pricing/postgres.rs`).

### Funding, welcome credit, and attribution

- New eligible OAuth B2C users receive exactly `$5.000000000`; historical grants retain the explicit
  `$4.000000000` legacy nominal (`packages/contracts/src/index.ts:2126`).
- Funding normalization and release-v2 reserve use bonus-first, then paid allocation.
- Settlement persists exact paid/bonus/other funding totals and lot lineage.
- Commerce derives referral eligibility from referred B2C plus positive `paid_funded_nano`, independent
  of pricing mode.
- Sales schema v2 stores immutable release/funding lineage, excludes welcome-funded usage, and combines
  v1/v2 history without rewriting old records.

### OpenKeys

- New issuance enforces `official_1_to_1` and `mult_bp=10000` at application and database boundaries.
- Multiplier/discount override fields are rejected before engine or database writes.
- Policy/funding reconciliation completes before a usable secret is returned.
- Failed issuance disables the unfinished engine account and keeps a durable reconciliation trail.

### Deployment state visible from GitHub

The audited SHA had green `deploy/tests`, `deploy/engine`, `deploy/backend`, `deploy/migration`,
`deploy/sales`, `deploy/openkeys`, `deploy/watchdog`, and Vercel contexts. Most bounded-context contexts
reported that the SHA contained no changes for that component. This establishes repository and selected
deployment health, not execution of the explicit pricing rollout jobs.

---

## Findings

### Critical — active progressive pricing remains a live product system

The target contract allows immutable historical rows but forbids active readers/writers of `track`, tier,
and retention semantics. Current code still runs them:

- `apps/worker/src/pricing-worker.service.ts:76` reconciles the tier ladder at startup;
- `apps/worker/src/pricing-worker.service.ts:118` refreshes and closes retention windows during normal
  polling;
- `packages/db/src/pricing.ts:1079` returns `pricingMode: "progressive"`, tier, retention/window spend,
  and next-tier data;
- `packages/contracts/src/index.ts:2115` exports the five-tier 60–70% ladder;
- `packages/contracts/src/index.ts:2131` accepts `pricingMode: "track"`;
- `packages/db/src/pricing-policy-write.ts:174` creates track/retention/commission eligibility records;
- authenticated web and admin surfaces still render and edit progressive semantics.

There is no release-head guard around the legacy worker mutations. The repository therefore has two live
pricing authorities rather than historical compatibility only. Definition-of-Done items 2 and 14 are not
met.

**Required remediation:** remove or hard-disable active legacy readers and writers, remove the
customer/admin controls and fields, and add a repository-wide regression proving that active code cannot
create or consume progressive semantics. Existing tier integration tests currently preserve the behavior
the target requires removing.

### High — final customer debit violates the floor contract

The contract requires:

```text
charged_nano = floor(official_nano * payable_multiplier_bp / 10000)
```

Reserve-time release-v2 hold correctly floors in
`crates/registry/src/pricing/release_v2.rs:647`. Final Anthropic, OpenAI, and Gemini settlement uses
`metering::apply_multiplier`, which adds `5000` before division and rounds half-up
(`crates/metering/src/lib.rs:458`; `crates/forward/src/meter.rs:509`;
`crates/forward/src/codex/billing.rs:841`; `crates/forward/src/gemini/billing.rs:970`). The registry
trusts this provider-adapter debit rather than recomputing it.

The difference is at most one nanoUSD per request, but it is an exact financial-contract violation. Stage
8 repeats the half-up calculation in `crates/registry/src/pricing/shadow.rs:1667`, so evidence can pass
while validating the wrong rule.

**Required remediation:** introduce one checked release-v2 floor helper and use it in reserve, all final
provider settlement paths, and Stage 8. Keep legacy half-up behavior only for immutable legacy snapshots
where required.

### Critical — customer/admin surfaces are not connected to release-v2 authority

The customer contract requires an effective discount and source:

```text
global_default | provider_override | model_override
```

Those identifiers exist only in the target document. The current customer projection supports only
provider/model rules, cannot represent a global rule, and exposes `track` and eligibility fields
(`packages/db/src/pricing.ts:1160`). `GET /account` still returns the legacy progressive projection
(`apps/api/src/account.service.ts:183`).

The generic admin editor likewise:

- cannot create or preview a global-scope rule;
- allows `track` for global B2C;
- represents service as editable discount rules rather than all-model `meter_only`;
- writes legacy managed policy heads/bindings.

Only the B2B-specific mutation explicitly synchronizes a newer policy into release-v2 assignment
extensions. A global B2C or service edit can therefore report success without changing the active
post-cutover release authority.

**Required remediation:** replace the customer projection with a release-v2-aware coherent read, expose
the exact rule source, and connect global B2C/service admin operations to versioned release-v2 authority.
This cannot be solved by frontend copy alone.

### High — OpenKeys admission authority is contradictory

`docs/commerce/MULTI-DISCOUNT.md:116` says every runtime-priceable provider/model automatically becomes
sellable by OpenKeys without a catalog cutover. Later sections and Stage 5/7 runbooks say the opposite:
OpenKeys uses an explicit catalog, and generation 5 deliberately excludes Gemini.

The implementation also contains both concepts:

- Stage 5 creates a fixed OpenKeys catalog and omits the Google product switch
  (`packages/db/src/pricing-stage5-materializer-v2.ts:483`);
- release-v2 model resolution bypasses OpenKeys catalog membership and permits a missing scoped OpenKeys
  switch (`crates/registry/src/pricing/postgres.rs:4496`, `:4557`).

KIMI and GLM have official tariffs and runtime billing paths but release-v2 quotes, the runtime resolver,
and Stage 8 accept only `anthropic | openai | google`. Consequently, the literal service “all runtime
models” and automatic OpenKeys-access decisions are not covered by the current release authority.

**Decision required:** make the product rule unambiguous. The safer recommendation is catalog-gated
product admission with fixed 1:1 pricing once admitted. If runtime-wide automatic admission remains the
owner decision, derive provider coverage from one capability manifest and implement/test KIMI and GLM in
release-v2, service `meter_only`, OpenKeys, and Stage 8.

### High — Sales v2 silently accepts conflicting immutable replays

Both pending and finalized v2 event inserts use `ON CONFLICT (commerce_event_id) DO NOTHING`
(`packages/sales-db/src/commissions-v2.ts:103`, `:123`). A replay with a different user, partner,
amount/funding composition, release/snapshot identity, or timestamp is accepted as buffered/duplicate
without comparing the persisted evidence.

The older v1 implementation performs the correct immutable-field comparison. The v2 behavior is
financially material: an incorrect first insert cannot be exposed or repaired by the authoritative retry.

**Required remediation:** on conflict, load and compare every immutable v2 field and fail the sync page on
any divergence. Cover both pending and finalized conflicts with PostgreSQL tests.

### High — mandatory test matrix is incomplete

Missing or insufficient proofs include:

- floor-versus-half-up settlement boundary cases;
- reserve before activation followed by **settlement** after activation (the inspected matrix proves
  cancellation after activation);
- customer global rule and `global_default | provider_override | model_override` projection;
- absence of all active tier/track/retention readers and writers;
- active-release service `meter_only` across every provider in scope;
- KIMI/GLM release-v2 coverage if the runtime-wide decision remains;
- conflicting Sales v2 replay for both pending and final events;
- one end-to-end Commerce ledger → HTTP feed → Sales commission test.

### Medium — durable rollout queues lack visible alerting

The funding-normalization, shadow-rollout, Stage 8 capture, and activation queues have durable status and
admin views, but the audit found no Prometheus alert coverage for retry/dead/backlog accumulation. An
operator may learn of a stalled control lane only by opening the pricing control room.

### Operational status — production completion is unverified

Source and green deployment contexts do not establish that production has:

- one blocker-free prepared Stage 5 target/recovery pair;
- a confirmed Stage 6 parent covering the full inventory;
- a confirmed Stage 7 rollout;
- fresh persisted `passed=true` Stage 8 evidence;
- a Stage 9 target activation receipt and exact active head;
- the post-activation B2C/B2B/OpenKeys/service/referral and cross-cutover monetary smoke.

This audit did not query the protected production status authorities. Absence of that evidence here does
not prove the stages were never run; it means Definition-of-Done items 9–13 and 15 cannot be certified.

---

## Design assessment

### Decisions that are sound

- Official tariff cost is computed first, followed by one non-stacking discount.
- Basis points and integer nanoUSD avoid floating-point money.
- Exact model → provider → global precedence is simple and understandable.
- B2B independence prevents accidental inheritance of a consumer promotion.
- OpenKeys 1:1 is an auditable invariant.
- Dedicated `meter_only` correctly separates service accounting from customer balance semantics.
- Bonus-first allocation provides honest paid/bonus attribution and referral commission basis.
- Immutable reserve-time snapshots allow old and new requests to coexist across cutover.
- Account-local normalization and one-head activation avoid an N-account mixed production state.
- Forward recovery is safer than attempting to restore an incompatible old runtime.

### Main design liability

The architecture lacks a completed authority handoff. Release-v2 is the intended final system, while the
legacy scalar/progressive policy system remains a first-class worker/API/UI system. This creates operator
ambiguity and makes “code deployed” materially different from “new economics active.” Additional
compatibility shims would worsen the risk; the next phase should consolidate authority and delete or
hard-fence the legacy active paths.

### Complexity assessment

The rollout is necessarily complex because it combines money, no downtime, no drain, complete inventory,
and reversible activation. The chosen primitives—immutable manifests, exact digests, short serializable
transactions, bounded durable workers, raw-first evidence, and lost-ACK replay—are appropriate. The
remaining complexity risk comes from duplicated provider/rounding authorities and very large DB modules,
not from the one-head rollout concept itself.

---

## Recommended remediation order

1. Fix floor arithmetic in final settlement and Stage 8.
2. Resolve the OpenKeys admission decision and KIMI/GLM scope.
3. Connect customer pricing and global/service admin operations to release-v2.
4. Remove or release-head-fence all active progressive readers and writers.
5. Reject conflicting Sales v2 replays.
6. Add the missing cross-cutover, provider, customer-source, cleanup, and Sales tests.
7. Add metrics/alerts for durable pricing-control queues.
8. Deploy the remediations and regenerate Stage 5 artifacts if immutable identities changed.
9. Inspect/execute production Stages 6–9 and preserve exact post-activation monetary evidence.

---

## Verification record

Passed locally:

```text
cargo test -p registry pricing::release_v2::tests --locked
5 passed

cargo test -p forward dormant_release_keeps --locked
3 passed
```

The three attempted focused TypeScript suites did not start because the isolated worktree had no local
`node_modules` (`vitest: command not found`):

```text
pnpm --filter @claude-api/db test
pnpm --filter @claude-api/sales-db test
pnpm --filter @claude-api/openkeys test
```

This is an audit-environment limitation, not a source-test failure. PostgreSQL integration matrices and
production control-plane mutations were not run. No product code or production state was modified by the
audit.
