# Customer pricing

The target contract was approved on 2026-08-02. Until the atomic Stage 9 cutover, production may
continue executing the old scalar/progressive path, but it must not be extended: the implementation
must arrive at the contract below following the zero-downtime plan in
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
`AccountService.ensureEngineAccount` (the first account access: dashboard, keys). The claim is
atomic (partial unique indexes) and the credit is idempotent by reference, so retries are safe and
do not double the bonus.

The welcome bonus can pay for any B2C-allowed Anthropic/OpenAI/Gemini model. Funding is spent
bonus-first, then paid. The nominal is shown as a money balance, without a marketing conversion
into "official usage".

## Provider/model pricing authority

Pricing policy and model admission are independent. The catalog includes a model in the product, a
switch can emergency-close a provider, and the policy sets the percentage. A missing applicable rule
after Stage 9 fails closed; scalar fallback is forbidden.

The current Gemini tariff schedule `google/gemini-developer-api/2026-08-02` includes
`gemini-3-flash-preview`: text/image/video input `$0.50/M`, audio input `$1/M`, cached text
`$0.05/M`, cached audio `$0.10/M`, output including thinking `$3/M` and Search `$14/1000 queries`.
The first controlled public-wire gate returned 404 with no output/usage, so immutable capability
generation 4 remains rejected and is never materialized. A fresh private-wire gate against the exact
implementation then completed 22 paid turns on Google AI Pro and Ultra: all thinking levels,
incremental SSE, cache, exact PCM audio and forced tools with terminal authoritative usage.
Publication proceeds through a new capability/catalog generation 5 and the production/public
allowlists. OpenKeys does not receive the model automatically: its generation 5 catalog keeps the
explicit Anthropic/OpenAI set.

Policy versions are immutable, content-addressed and delivered catalog → switches → policy. All
accounts switch via a single active release head, not a sequential update of bindings.

## B2B

B2B does not inherit the global B2C policy or its provider/model overrides. The client has its own
immutable policy, thereafter edited by full CAS replacement. The policy originates from exactly one
of two paths: invitation redemption copies the invitation snapshot at registration, while a manual
admin conversion of an existing B2C customer provisions a fresh policy with a single Anthropic
discount rule derived from the negotiated multiplier and re-points the customer's single account
binding (the Stage 5 backfill already bound it to the global B2C policy) at that policy.
Re-running the conversion on a customer who
is already B2B repairs a missing policy (customers converted before this provisioning existed)
against the multiplier already in effect, and is otherwise a no-op — it never rewrites an existing
policy or the active scalar.

The legacy engine delivery lane keeps an immutable policy lineage per strict account and rejects
any prepare that switches identity (global B2C → client policy) with `version_conflict`; a shadow
account accepts the same switch as a shadow rebind pre-cutover. Conversion and later policy edits
therefore stage the identity switch as a normal delivery for the backfilled shadow binding, while
for a strict binding nothing is staged: the engine keeps running the confirmed lineage, the scalar
multiplier stays authoritative, and any drifted staged-but-undeliverable desired state is folded
back to the engine-confirmed applied state. Until the cutover the scalar remains the only
engine-enforced price for shadow accounts too.

## Per-account strict cutover

The default strict transition is the fleet-wide Stage 9 release CAS (see
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`). One individually negotiated B2B client can be cut
over earlier through `POST /v1/admin/users/:id/policy-enforcement-cutover`
(`AdminOperationsService.cutoverUserPolicyToStrict`), which makes the client's confirmed
per-provider policy the enforced price instead of the legacy scalar. The endpoint orchestrates
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

The existing scalar `mult_bp` becomes only an Anthropic provider rule at migration:

```text
provider_id = anthropic
discount_bps = 10000 - mult_bp
```

OpenAI/Gemini do not appear for an existing B2B automatically. The operator adds them via explicit
provider/model rules. Until the release cutover the engine enforces only the legacy scalar, so a
saved b2b_client policy whose rules are a uniform set of provider-level discounts also moves the
scalar (`mult_bp = 10000 - discount_bps` on `customer_profiles` and `engine_accounts`, plus the
durable scalar delivery job) — otherwise the panel would show the new policy while billing follows
the stale multiplier. A policy that cannot be one scalar (mixed scopes, track rules, per-provider
differences) leaves the scalar untouched and activates only with the cutover. The customer
dashboard prices each provider by the same honesty rule: while the binding's policy is not
engine-enforced (`legacy_scalar`/`shadow`), the usage page shows the materialized per-provider
policy discount clamped to never exceed the discount the legacy scalar actually bills — a
tighter negotiated provider rate (say 60% on Google against a 70% scalar) shows as configured,
a looser one shows the scalar. Billing can only over-deliver against the badge until the
cutover, never overcharge it; providers the policy does not cover stay unavailable exactly as
the materialized rules say.

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
referral commission. A new model requires explicit OpenKeys catalog enablement.

## Service

Service accounts have `billing_mode=meter_only`: all runtime-capable models are available, official
usage and tariff lineage are preserved, but no balance reserve/debit is performed and a zero balance
does not produce a 402. Restrictions of a specific domain live in that domain's code, not in the
pricing policy.

## Referral commission

Referral eligibility no longer depends on the pricing mode. For referred B2C settlement, commerce
passes the exact `paid_funded_nano`; the bonus-funded portion, B2B, OpenKeys and service are
excluded. Sales applies the existing `commission_bps`/`sub_commission_bps` to this integer base.

Immutable ledger attribution must contain pricing release/policy/rule/tariff identities, official
and charged cost, ordered funding allocations and exact paid/bonus totals. Commerce validates the
evidence and cursor in one transaction; the sales feed receives only the confirmed event.

## Zero-downtime activation

A new policy is not activated per account. Dual-compatible runtime and funding writers are first
rolled out dormant, funding is normalized online by account-local transactions, and the
full-inventory shadow runs on 100% of traffic. Stage 9 then changes the global active release head
with a single CAS.

Active v2 reservations may cross the cutover and settle against the immutable reserve snapshot. A
global drain, maintenance window and canary-account rollout are forbidden. The full runbook is
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`.

## Known temporary gap

Until the rollout completes, the old code/schema may still contain tiers, retention, `track`, the
`$4` grant and scalar jobs. This is the migration source, not the target contract. New code must not
add functionality to them. After the switchover the readers/writers are removed; immutable history
and already-applied append-only migrations are not rewritten.
