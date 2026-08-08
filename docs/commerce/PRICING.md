# Customer pricing

The contract below was approved on 2026-08-02 and is LIVE: the Stage 9 one-head CAS activated
the target pricing release on 2026-08-04 (engine `pricing_release_head_v2`, generation 13), and
the release-v2 authority governs admission and pricing for every account class. The retired
scalar/progressive path survives only as immutable history and the cleanup surface tracked in
`docs/commerce/MULTI-DISCOUNT.md`.

## B2C

A regular B2C customer pays 50% of the official cost of any model in the main product catalog:

```text
global discount_bps = 5000
global payable_multiplier_bp = 5000
```

The target system has no progressive tiers, top-up thresholds, 30-day retention or month-close
pricing behavior. A top-up only increases the balance and does not change the discount percentage.

An operator may set a B2C provider/model override. The priority is always:

1. exact model rule;
2. provider rule;
3. global 50%.

For example, Gemini 60% plus a separate Gemini image model at 55% yields 55% for exactly the image
model and 60% for all other Gemini models. Discounts do not stack.

The global B2C policy is pinned by the active pricing release. Post-cutover a direct editor save
is refused with `release_cycle_required`: changing the global rule set requires a new release
cycle (new prepared target/recovery pair, fresh evidence, one head CAS) so the panel can never
diverge from enforced prices. Service policies are pinned the same way: the release authority
runs service as `meter_only` without rules, so a post-cutover service editor save is refused with
the same `release_cycle_required` instead of silently versioning a legacy document.

The official cost is computed by `crates/metering` from the immutable effective-dated tariff and
only then multiplied by the integer `payable_multiplier_bp`. All amounts are nanoUSD/decimal
strings; float and JavaScript `number` are forbidden for money.

## Welcome bonus

A new eligible Google/GitHub B2C registration receives exactly `$5.000000000` with the idempotent
`signup-bonus:<commercial-user-id>`. Password, invited B2B, OpenKeys and service accounts do not
receive the bonus. Previously issued `$4` grants are retained without retroactive increase.

The anti-fraud profile and flags are always recorded at OAuth sign-in, while the claim happens only
against an engine account confirmed `active` by a fresh database read. Under managed pricing the
account is activated by the worker asynchronously after registration — in that case the claim is
deferred and retried from two points: the next OAuth sign-in and
`AccountService.ensureEngineAccount` (the first account access: dashboard, keys). Both settlement
points require a Google/GitHub identity in `auth_identities`, so a password registration never
passes the deferred gate even with a clean anti-fraud profile. The claim is
atomic (partial unique indexes) and the credit is idempotent by reference, so retries are safe and
do not double the bonus.

The welcome bonus can pay for any B2C-allowed Anthropic/OpenAI/Gemini model. Funding is spent
bonus-first, then paid. The nominal is shown as a money balance, without a marketing conversion
into "official usage".

## Provider/model pricing authority

Pricing policy and model admission are independent. The catalog includes a model in the product, a
switch can emergency-close a provider, and the policy sets the percentage. A missing applicable
rule fails closed; scalar fallback is forbidden.

The current Gemini tariff schedule `google/gemini-developer-api/2026-08-02` includes
`gemini-3-flash-preview`: text/image/video input `$0.50/M`, audio input `$1/M`, cached text
`$0.05/M`, cached audio `$0.10/M`, output including thinking `$3/M` and Search `$14/1000 queries`.
The first controlled public-wire gate returned 404 with no output/usage, so immutable capability
generation 4 remains rejected and is never materialized. A fresh private-wire gate against the exact
implementation then completed 22 paid turns on Google AI Pro and Ultra: all thinking levels,
incremental SSE, cache, exact PCM audio and forced tools with terminal authoritative usage.
Publication proceeds through capability/catalog generation 5 and the production/public allowlists.
OpenKeys admits Gemini at 1:1 like every other runtime-priceable model (see the OpenKeys section).
Generation 6 adds only the tariff-pinned
`openai/gpt-image-2-2026-04-21` snapshot to main and OpenKeys; it activated as the first
successor advance of the release head (13 -> 41) after the public production generation+edit
smoke turned overall watchdog-GREEN. There is still no reseller, separate image API key, or
fallback: the model rides the existing sealed Codex OAuth pool, and its customer contract stays
the narrow proved shape (`background=opaque`, `quality=low`, `size=auto`, one PNG).
GPT Image 2 meters five disjoint legs (fresh/cached text input, fresh/cached **image** input on
edits, and image output). The unified `/v1/models` rate card is the closed OpenAI-compatible schema
v1 shape, which has fields only for input, cached input, cache write and output, so it publishes
text input / cached text input / image output; the image-input legs are billed at their own audited
rate from `metering::openai_image` (`$8/M` fresh, `$2/M` cached) and are visible in settlement, not
in that card. Folding them into the published `input` leg would misprice plain generation, which
has no image input at all.
Generation 7 closes the Claude catalog hole: `claude-opus-4-6`, `claude-opus-4-5` and
`claude-sonnet-4-5` join main and OpenKeys with their existing metering tariffs, and the dated
snapshots (`claude-opus-4-5-20251101`, `claude-sonnet-4-5-20250929`, `claude-haiku-4-5-20251001`)
keep resolving through the metering normalization onto exactly these canonical identities.

Policy versions are immutable, content-addressed and delivered catalog → switches → policy. All
accounts switch via a single active release head, not a sequential update of bindings.

## B2B

B2B does not inherit the global B2C policy or its provider/model overrides. The client has its own
immutable policy, thereafter edited by full CAS replacement. The policy originates from exactly one
of two paths: invitation redemption copies the invitation snapshot at registration, while a manual
admin conversion of an existing B2C customer provisions a fresh policy with a single Anthropic
discount rule derived from the negotiated multiplier and re-points the customer's single account
binding (the Stage 5 backfill already bound it to the global B2C policy) at that policy.
Re-running the conversion on a customer who is already B2B repairs a missing policy (customers
converted before this provisioning existed) against the multiplier already in effect, and is
otherwise a no-op — it never rewrites an existing policy.

Conversion and every later B2B policy save are enforced per account. Post-cutover there are
two lanes, chosen by the account's commerce binding. For a release-covered (shadow/legacy)
account the enforcement lane is the append-only assignment extension: the admin sync
(`syncPricingReleasePolicyOverrideV2` in `packages/db/src/pricing-release-override-v2.ts`)
prepares the account's release policy — a strictly newer
version for an already-B2B base, or the first version of the new per-account B2B lineage for a
converted B2C base — and pins it for the exact active head and its paired recovery, and the
runtime resolver prefers the extension over the immutable base assignment. For a strict account
(a new-account direct strict chain graduate — an opted-out account — or a pre-cutover strict
conversion) the save rides the policy_v1 delivery lane instead: the strict→strict delivery is
staged by `materializeBinding` in the same policy-save transaction, and the sync reports
`policy_owned` without touching the release authority. The conversion API
reports the propagation outcome in its `release_v2` response field. The class-changing extension
for a converted release-covered account is written only by this admin sync / conversion lane —
key issuance no longer self-heals through the extension. The
client's own policy is therefore the only
enforced price for new charges and every B2C input — the global default, its provider/model
overrides, the legacy scalar, any progressive remnant — is dead for the account. The flip never
rewrites the immutable ledger and never reprices an in-flight reservation: those settle on
their reserve-time pinned snapshots. Remaining welcome-bonus funds stay spendable under the
B2B policy (funding, not pricing), and B2B charges carry no referral commission basis.

The customer dashboard projection (`getCustomerPricingPolicyView`) follows the same authority:
post-cutover it presents a B2B account's pinned policy rules exactly as configured — never
clamped to the retired legacy scalar — and a B2C account as the managed global policy head
(the content the active release pins; a direct global save is refused with
`release_cycle_required`, so the two cannot diverge). Before the cutover, a not-yet-strict B2B
badge was clamped to the scalar that legacy billing actually applied; that clamp is pre-cutover
semantics and is inactive once the cutover receipt is durable.

Before the fleet cutover the same contract ran through the per-account strict lane: conversion
and saves armed `strict_chain_pending`, and the pricing worker chained funding normalization,
exact key ACK stamps, and the atomic strict+strict+verified delivery once the exact client
policy version was shadow-confirmed and reconciliation-verified. Once the cutover receipt
became durable the B2B lane stood down: conversion and save writers no longer arm the flag, and
the manual staging entry point `stageAccountStrictCutoverJob` refuses with `post_cutover`. The
flag itself was re-purposed for the release-v2 retirement (phase 2.1): registration
provisioning (`materializeProvisionedUserPolicy` in `packages/db/src/pricing-policy-write.ts`)
arms `strict_chain_pending` on the fresh binding, and the pricing worker drives the new-account
direct strict chain (`packages/db/src/strict-chain.ts`) — once the materialized policy version
confirms under shadow, the shared engine preflight (funding normalization + active-key ACK
stamps) runs and the atomic strict/strict/verified delivery is staged via
`stageProvisionedAccountStrictJob`; once the strict delivery confirms, the worker writes the
one-way engine opt-out marker and disarms the flag. The chain is fail-closed: a precondition
that cannot advance is recorded on the binding and retried, and the account is never opted out
before its strict path is live.

The legacy engine delivery lane keeps an immutable policy lineage per strict account and rejects
any prepare that switches identity (global B2C → client policy) with `version_conflict`; a shadow
account accepts the same switch as a shadow rebind pre-cutover. Any drifted
staged-but-undeliverable legacy desired state is folded back to the
engine-confirmed applied state.

## Per-account strict cutover (pre-cutover lane, stood down)

The fleet-wide Stage 9 release CAS completed on 2026-08-04 (see
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`), so this lane is stood down: the conversion/save
writers stop arming
`strict_chain_pending`, and the manual
endpoint `POST /v1/admin/users/:id/policy-enforcement-cutover` (via
`stageAccountStrictCutoverJob`) refuses with `post_cutover`.
Post-cutover per-account B2B enforcement belongs to the assignment extension lane described
above, and the flag now belongs to the new-account direct strict chain (previous section). The mechanics below remain as the record of how the lane enforced one individually
negotiated B2B client ahead of the fleet CAS — it made the client's confirmed
per-provider policy the enforced price instead of the legacy scalar by orchestrating
the full precondition set the engine enforces atomically at the flip:

1. funding buckets are normalized to equal the account aggregates
   (`GET|POST /admin/pricing/v2/funding/{account}/normalization`; a blocked plan aborts with 409);
2. every active API key is stamped with the exact active-policy ACK
   (`POST /admin/key-id/{key_id}/status` with `activation_policy_ack`) — the cutover trigger
   rejects the flip over an unstamped key;
3. the durable strict control job is staged via `stageAccountStrictCutoverJob`
   (strict policy + strict funding + verified, targeting exactly the shadow-confirmed version).

The worker delivers the activation and immediately re-stamps the account's active keys with the
new head — request auth on a strict account admits only keys stamped with the current policy
version+digest, so without that hook every strict activation (the cutover and each later policy
save) would break the client's keys. Key issue attaches the ACK too: `createApiKey` sends
`activation_policy_ack` whenever the account is strict. A strict→strict policy advance keeps
keys working through the same re-stamp in the delivery job; the engine identity stays frozen
(no rebind) once strict. The endpoint is idempotent — a replay reports `already_strict` with the
original job id and status instead of staging a duplicate. Engine-side guards that cannot be
pre-flighted from commerce (drained legacy reservations, policy-capable engine instances) fail
the delivery job loudly with the trigger error, never a partial state.

The retired scalar `mult_bp` became only an Anthropic provider rule at migration:

```text
provider_id = anthropic
discount_bps = 10000 - mult_bp
```

OpenAI/Gemini do not appear for an existing B2B automatically. The operator adds them via explicit
provider/model rules; post-cutover every save propagates through the assignment extension lane for
any rule shape, so a mixed-scope policy is enforced as soon as the extension is pinned and never
waits for a fleet transition. The legacy scalar no longer prices anything: it survives only as
immutable history and as the migration source for the initial Anthropic rule.

B2B spend IS ingested by the pricing usage sync: `listPricingSyncTargets` selects both
`b2c` and `b2b`, so every charge lands in the immutable `pricing_usage_events` (with provider
attribution and provider backfill) and the admin control room reports real B2B numbers. The
progressive B2C machinery never touches B2B: `applyPricingLedgerPage` skips the free-first
projection, `pricing_months` and the tier-window counters for it, and a pre-attribution B2B charge
creates no commission basis (`real_funded_nano = 0`) because there is no local funding projection
to prove paid money — under-paying commission is safe, over-paying is not.

## OpenKeys

All existing and new OpenKeys operate 1:1: `discount_bps=0`,
`payable_multiplier_bp=10000`. They do not inherit B2C/B2B discounts and do not participate in
referral commission. Access follows the runtime: every provider and model the engine can price —
Anthropic, OpenAI, and Google Gemini included — is sellable at 1:1, and newly admitted providers
flow automatically. The master switch and an explicitly disabled scoped provider switch still
close a provider, and a model without a runtime tariff fails closed at quote time.

## Service

Service accounts have `billing_mode=meter_only`: all runtime-capable models are available, official
usage and tariff lineage are preserved, but no balance reserve/debit is performed and a zero balance
does not produce a 402. Restrictions of a specific domain live in that domain's code, not in the
pricing policy. Service accounts deliberately stay on the release path in the release-v2 retirement
(phases 2.1–2.2): the engine has no meter-only lane outside release-v2 — a rule-free strict policy admits
nothing, managed discounts cap at 9500 bps so payable 0 is impossible, the legacy scalar lane cannot
express meter-only and rejects zero-balance accounts, and an uncovered non-opted-out account is not
served at all — so `ensureServicePricingReleaseProvisioningV2` keeps completing their exact
`meter_only` policy/extension until an engine meter-only lane exists (phase 3); the phase 2.2
backfill deliberately excludes them (`docs/ops/PRICING_RELEASE_BACKFILL.md`).

## Referral commission

Referral eligibility no longer depends on the pricing mode. For referred B2C settlement, commerce
passes the exact `paid_funded_nano`; the bonus-funded portion, B2B, OpenKeys and service are
excluded. Sales applies the existing `commission_bps`/`sub_commission_bps` to this integer base.

Immutable ledger attribution must contain pricing release/policy/rule/tariff identities, official
and charged cost, ordered funding allocations and exact paid/bonus totals. Commerce validates the
evidence and cursor in one transaction; the sales feed receives only the confirmed event.

## Post-cutover operation

The fleet cutover completed on 2026-08-04 (release generation 13, one head CAS, no traffic
stop). New accounts complete the direct strict chain before a usable key is issued: a
strict/strict/verified binding, a key born with the exact activation ACK, and the one-way
`pricing_release_opt_out_ts` marker; existing accounts keep their release coverage until the
backfill lane retires them (phase 2.2, below). B2B policy saves propagate through the
assignment extension lane for release-covered accounts and through the policy_v1 delivery
lane (`policy_owned`) for opted-out
accounts. Reservations started
before the head CAS settled against their immutable reserve-time snapshots; legacy-format
outbox rows completed without a drain. The recovery path is a forward CAS to the paired
recovery generation, never a rollback to an old binary. The full runbook and evidence chain:
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`.

## Existing-account backfill (release-v2 retirement, phase 2.2)

The pre-existing fleet moves to the direct strict path through a bounded, resumable,
canary-first backfill — never a bulk flip. The commerce lane lives in
`packages/db/src/pricing-backfill.ts` and runs on the pricing worker's slow sweep
(`PRICING_BACKFILL_ENABLED`, `PRICING_BACKFILL_BATCH_SIZE` default 5/pass,
`PRICING_BACKFILL_ACCOUNT_ALLOWLIST` for canary mode). Per account it (1) aligns the dormant
engine scalar (`accounts.mult_bp`, unread by the release path but the strict admission's
fallback for rule-less scopes) to the fallback DERIVED from the live release policy —
B2C 5000 by engine validation, B2B full price (release B2B policies cannot carry a global
rule) — via an idempotent `account_set_mult_bp` plus commerce mirror sync, (2) re-materializes
the managed policy at the live catalog head through the registration writer with arming
disabled (B2C — the current `policy:main:global-b2c` head; B2B — the account's own
`b2b_client` policy, per-model/provider scopes preserved exactly), (3) proves the mechanical
equivalence — the release-side resolution (assignment extension over base, model → provider
→ global) must equal the strict policy's payable multiplier at every scope, and the engine
scalar must observably equal the aligned fallback; B2C is the
5000-global identity, B2B is exact scope-set equality over normalized scope→payable maps
(stored digests are never compared across the frozen v1/v2 domains) — and (4) arms the same
`strict_chain_pending` direct chain new accounts use, which finishes with the one-way engine
opt-out marker and a durable `pricing_release.opt_out` audit entry (the terminal "done" for
candidate selection and the `pricing_backfill` section of `GET /v1/admin/pipeline-health`).
A mismatch is skipped with `last_error`, never forced; per-account failures are isolated.
Pre-existing OpenKeys accounts are swept by the bounded internal admin endpoint
`POST /api/internal/admin/strict-backfill` in `apps/openkeys` (official 1:1 strict policy,
key ACK, opt-out — idempotent). Service accounts intentionally remain on the release path
(see "Service"). Ops sequence: `docs/ops/PRICING_RELEASE_BACKFILL.md`.

## Retired progressive machinery

The progressive model (tiers, retention windows, `track`, referral price floors, the free-first
projection) was removed from active code on 2026-08-06 under §6 of
`docs/commerce/MULTI-DISCOUNT.md`: no writer creates progressive records and no reader consumes
them for admission, pricing, funding, or commissions. Concretely: the worker no longer runs tier
reconciliation/window/month-close jobs; registration no longer creates `pricing_months` rows or
tier-0 scalars (new B2C accounts get the flat 50% placeholder); the usage sync keeps only the
immutable engine-evidence commission basis (unattributed rows get none — under-paying is the
safe direction); the customer pricing summary is flat; and `setReferralFloor` keeps only the
partner attribution record without touching any multiplier. Immutable history, applied
migrations, and the legacy columns remain as audit data; their physical removal is a separate
late change after a proven absence of readers/writers.
