# Pricing: one balance, one discount

Status: live contract as of 2026-08-10. This document replaces the per-account policy/catalog/
switch/release machinery described in `MULTI-DISCOUNT.md` and its stage documents, which are kept
only as history.

## The model

An account has:

- **a balance** — `accounts.balance_nano`, whole nanoUSD, the single authority on what the customer
  can spend;
- **a discount** — `accounts.mult_bp`, a payable multiplier in basis points (10000 = list price,
  5000 = 50% off, 0 = free);
- **optional per-provider discounts** — rows in `account_provider_discounts (account_id,
  provider_id, mult_bp)`. A provider with no row is priced by the account default.

That is the whole policy. There is no catalog, no switch, no rule lineage, no release generation,
no shadow evaluation and no second representation of the customer's money.

- **B2C**: the account default (today 50%) and no provider rows.
- **B2B**: the account default plus a row for each provider whose terms were negotiated
  separately. Different providers may carry different discounts for the same customer.
- **OpenKeys**: `mult_bp = 10000`.
- **Service/internal**: `mult_bp = 0` — metered as usual, holds and charges are zero.

Every model of every provider is available to every key. Availability is a runtime question
(is the provider enabled, does the pool have capacity), never a pricing one.

## How a request is priced

1. `authorize` resolves the key to its account and reads the balance, the default discount and the
   account's provider overrides (`KeyAuth::provider_mult_bp`).
2. The provider plane asks `Authz::mult_for(provider_id)` — the override if the account has one,
   otherwise the default.
3. The provider plane resolves and pins the official effective-dated tariff for the request's
   priced timestamp. Gemini 3.6 Flash, for example, pins the $0.75 / $0.075 / $3.75 per-1M promo
   through 2026-12-31 and $1.50 / $0.15 / $7.50 from 2027-01-01T00:00:00Z; Search stays on its
   separate per-query leg. The storefront uses the same effective-date contract at build time. This
   provider tariff is not an account discount.
4. Admission caps the reserve to the balance (`cap_to_balance`) and reserves atomically.
5. Settlement charges the real usage at the same tariff and multiplier the reserve pinned, against the same
   balance.

A discount write is live on the next request. There is no version to activate, nothing to
materialize and nothing that can disagree with the balance.

## Control API

| Route | Meaning |
|---|---|
| `POST /admin/account/{id}/pricing` `{mult_bp}` | Set the account default discount. |
| `GET /admin/account/{id}/discounts` | The default plus every provider override. |
| `POST /admin/account/{id}/discounts` `{provider_id, mult_bp}` | Set one provider override; `mult_bp: null` removes it. |

`provider_id` is one of `anthropic`, `openai`, `google`, `kimi`, `glm`; `mult_bp` is `0..=10000`.
The write APIs and both PostgreSQL authorities enforce the closed provider/range contract; commerce
also fences its account mirror at the same upper bound. An unknown provider id would silently never
match a request, and an out-of-range multiplier is either free inference or an overcharge. `zhipu`
appears only as a vendor namespace in GLM tariff/calibration identities, never as a pricing provider
id; the `zhipu` word in engine migration 0043's historical comment was corrected by the additive
0046 constraint rather than by rewriting an applied migration.

## The commerce side

Commerce records what was asked for and makes delivery durable; the engine remains the authority
that prices requests.

- `customer_profiles.multiplier_bp` — the customer's default discount.
- `customer_provider_discounts (user_id, provider_id, multiplier_bp)` — their per-provider terms.
- `engine_pricing_jobs` — one row per (user, target). `provider_id IS NULL` delivers the default,
  a provider id delivers that override, and a null multiplier on a provider job removes it. A
  default change and a provider change are independent deliveries and never evict one another.

Read surfaces return the persisted scalar for both B2B and B2C. They never substitute the common
B2C value (`5000` today): dormant/unprovisioned historical accounts can legitimately carry another
stored value, and hiding it makes operator reconciliation impossible without direct SQL.

The worker claims a job, calls the engine and confirms. If the desired value moved while the
delivery was in flight, the job is requeued with the new value rather than confirmed, so an edit
made during an engine outage is delayed, never lost.

Every terminal write fences on the lease the worker was given (`locked_by` plus the monotonic
`attempts`), not merely on `status = 'processing'`. A worker whose lease expired can still be
alive and still finish its HTTP call; without the fence its late verdict landed on the delivery
that had already replaced it, clearing the new owner's lease and marking the job confirmed while
the value that owner was sending had never reached the engine. Losing the fence race is silent by
design — the job belongs to whoever re-claimed it.

The desired-state comparison and terminal job write are one transaction. Confirmation locks the
same customer/account authority rows that every pricing edit locks, in the same order, before it
reads the desired value and then updates the job. This closes the commit-order race where a worker
could read the old desired value, wait behind an administrator's job-row lock, and mark that old
payload confirmed immediately after the new deal committed. Whichever side gets the authority lock
first wins cleanly: either the old delivery confirms before the edit and the edit requeues it, or
the edit commits first and the worker requeues the new value.

Admin surface: `PATCH /admin/business-users/{id}/pricing` accepts `discountPercent` and/or
`providers` (a provider mapped to `null` clears its override), and `GET` returns the default
(`discountPercent`, `multiplierBp`) together with the current overrides. The panel shows the
default and one field per provider; an empty field means "use the default".

One PATCH is one deal, so it commits as one transaction (`setBusinessPricingBundle`): the default
and every override land together or not at all, and each target still gets its own delivery job.
Writing them as separate transactions — the shape until 2026-08-10 — left a window in which the
customer was priced by half of the new terms, and a failure partway made that window permanent.
The `GET` used to omit the default entirely, so an operator could not see the rate they were about
to replace; four live per-provider overrides written straight to the engine were invisible in that
view and one careless save would have dropped them.

The Sales partner boundary uses the same desired-state and delivery primitives through
`applySalesPartnerBusinessPricing`, with one additional fence: a stable operation ref is serialized
and recorded as terminal `audit_log` evidence in the transaction that converts B2C→B2B and writes
all default/provider terms. Exact retry returns that stored result; a different payload under the
same ref is a conflict. This closes both the partial-conversion window and the lost-response replay
window without reviving a policy or release table.

Invitations carry a discount percent, which the invitee's account is created with. Per-provider
terms are set on the client after conversion.

## Why the previous design was removed

The retired design put a second representation of both price and money in front of admission: a
per-account policy binding with `strict` enforcement, priced through an immutable catalog/switch/
release lineage, funded from `funding_buckets` rather than the account balance.

On 2026-08-09 the two representations disagreed. Accounts were moved to `strict` enforcement by a
commerce sweep whose preflight normalized funding into `funding_lots_v2`, while the engine's strict
admission read the older `funding_buckets`. For **166 of 168** accounts that table was empty, so
`paid_available_nano` resolved to 0 and every request was refused with
`402 insufficient balance or key spending limit reached for this request` — while the account
balance was intact and visible in the customer's dashboard. 72 of those accounts were funded, and
$6,865 of customer balance was unspendable. The failure was silent by construction: the refusal is
a legitimate account-state error, so nothing alerted.

The lesson is in the shape, not the bug: a discount is a number attached to an account, and money
has exactly one authority. Any design that gives either of them a second source of truth can drift,
and the drift surfaces as a customer being told they have no money.

## Invariants

1. **One authority for money.** The account balance. Nothing else may gate admission.
2. **One authority for price.** The account default, overridden per provider. Nothing derives a
   multiplier from anywhere else.
3. **A discount write takes effect on the next request.** No generation, no activation, no
   snapshot to keep in step.
4. **A funded account can spend.** If a request is refused for money, the account balance must
   actually be insufficient for the hold plus the one account-wide $1 admission buffer. PostgreSQL
   and the SQLite audit/fallback implementation apply the same post-reserve floor.

## Spend accounting: free money is spent first

A charge is money the customer spent; part of it may have been covered by free credit (welcome
bonus, promo, admin credit). That split has exactly one writer:

- `customer_profiles.free_balance_nano` — the durable free balance. A top-up whose engine `ref`
  is not a payment-provider reference credits it (`isFreeCreditRef`).
- `pricing_usage_events.real_funded_nano` — the part of that charge the customer paid for. Free
  balance is consumed first; the remainder is real.

Three surfaces read it, and nothing else may reimplement the split:

- **Partner commission.** The sales feed emits `real_funded_nano` as the commission basis, so free
  credit never becomes commission.
- **Refund eligibility.** A top-up is refundable only while no real money has been spent since it.
- **CRM referral profiles.** The scoped bridge sums `real_funded_nano` only for registrations reached
  through post-scalar CRM aliases; it never falls back to retired pricing-attribution evidence.

`admin-credit:*` is deliberately free/commission-ineligible. The admin audit reason “manual action”
does not prove an external payment, and an irreversible partner payout must not be created from an
unverified source. If off-platform B2B payments become a supported product, they need a separate
typed reference plus durable payment evidence; reinterpreting an ordinary admin credit is forbidden.
The admin finance projection follows the same boundary: new `admin-credit:*` top-ups are recorded as
`source='bonus'`, and historical immutable rows originally classified as `manual` are excluded from
manual revenue and paying-user cohorts by their ref. The rows are evidence and are never rewritten.

## Refund and chargeback compensation

The provider's terminal refund event and its engine compensation are one durable workflow:

1. Commerce locks the checkout, payment and positive `engine_credits` row in one transaction.
2. If that credit was never claimed, it becomes `dead` and no engine debit is necessary.
3. If an attempt may have reached the engine (retry/processing/confirmed), commerce atomically
   marks the payment refunded and inserts one negative `engine_adjustments` row keyed by
   `refund:<payment-id>`. A previously operator-dead attempted credit is revived so the sequence
   can be established rather than guessed.
4. The adjustment worker can claim only after the paired positive credit is `confirmed`; it then
   calls the idempotent engine debit and confirms through its unique fenced lease. Lost responses,
   expired leases and process restarts replay safely.
5. The same terminal transaction appends one immutable `payment.reversed` audit row for a payment
   that previously existed. `GET /v1/internal/sales/payment-reversals` publishes these rows under a
   commit-ordered cursor. This is partner-accounting evidence only: it never mutates usage history
   or engine money, and an unreferred customer's reversal advances the source watermark without
   crossing the Sales boundary.

This order intentionally proves “top-up once, compensate once”. Debiting immediately after an
ambiguous credit timeout could subtract money that was never added; canceling the retry could leave
a top-up whose successful response was lost. Queue state is exported through the existing
`engine_adjustments` monitoring series and the admin refund table shows its terminal status.
Provider chargebacks normalized by an adapter to the terminal `refunded` state follow the same
workflow and currently carry `kind=refund`; the expand-only wire enum also reserves `dispute` for a
future provider that can prove that distinction directly.

## What was removed, and what remains

The retired code is gone from every runtime path: the per-account policy/binding resolver, the
immutable catalog/switch/policy versions, release-v2, the funding buckets and lots, the shadow
evaluation authority, the bridge sampler, the strict activation ACK on key writes, the per-request
policy attribution on the ledger, and the paid/bonus funding split on the account read.

Two consequences worth knowing:

- `GET /admin/account/{id}` returns the account. The `funding` object and the per-entry
  `funding_allocations`/`attribution` on the ledger are gone — they were computed from
  `funding_buckets`, which only strict accounts ever populated, so they had no writer left.
- A zero multiplier is now the whole meaning of "free but metered": it holds nothing, charges
  nothing, and still records the usage row.

The tables themselves (`account_policy_bindings`, `funding_buckets`, `funding_lots_v2`,
`pricing_*`, `pricing_usage_attributions`) are immutable incident evidence until every retention,
rollback, watermark, dependency, backup and health gate in `docs/ops/PRICING_RETIREMENT.md` passes.
The runbook pins the exact engine/commerce manifests and forward-only drop order. Until that staged
contraction is GREEN, nothing may start reading them again.

## The 2026-08-09 aftermath: B2B discounts

The B2B discount lived in `account_policy_rules.discount_bps` while `accounts.mult_bp` held a
neutral 10000. When the policy lane stopped being consulted, all nine B2B accounts silently fell
back to full list price; one of them was billed 629 requests at 100% instead of 15% before it was
caught. Their negotiated rates were recovered from the active policy version and written to
`accounts.mult_bp`, and the commerce worker's strict-chain/backfill lanes — which kept resetting
the scalar to 10000 — were disarmed (`PRICING_BACKFILL_ENABLED=false`) and then deleted.

Two affected accounts required exact Google/OpenAI terms in addition to their defaults. Their four
provider rows are now present in both commerce desired state and the engine authority; the
cross-database default/provider/status reconciliation is the production guard against losing them
on a later admin save. Customer identities and negotiated values belong in protected audit data,
not this repository document.
