# Pricing: one balance, one discount

Status: live contract as of 2026-08-09. This document replaces the per-account policy/catalog/
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
3. Admission caps the reserve to the balance (`cap_to_balance`) and reserves atomically.
4. Settlement charges the real usage at the same multiplier the reserve pinned, against the same
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
Both bounds are enforced by the engine — an unknown provider id would silently never match a
request, and an out-of-range multiplier is either free inference or an overcharge.

## The commerce side

Commerce records what was asked for and makes delivery durable; the engine remains the authority
that prices requests.

- `customer_profiles.multiplier_bp` — the customer's default discount.
- `customer_provider_discounts (user_id, provider_id, multiplier_bp)` — their per-provider terms.
- `engine_pricing_jobs` — one row per (user, target). `provider_id IS NULL` delivers the default,
  a provider id delivers that override, and a null multiplier on a provider job removes it. A
  default change and a provider change are independent deliveries and never evict one another.

The worker claims a job, calls the engine and confirms. If the desired value moved while the
delivery was in flight, the job is requeued with the new value rather than confirmed, so an edit
made during an engine outage is delayed, never lost.

Admin surface: `PATCH /admin/business-users/{id}/pricing` accepts `discountPercent` and/or
`providers` (a provider mapped to `null` clears its override), `GET` returns the current
overrides. The panel shows the default and one field per provider; an empty field means "use the
default".

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
   actually be insufficient for the hold.

## Still to remove

`crates/registry` keeps the persistence of the retired design — the policy/catalog/switch/release
tables and their code, plus `funding_v2`. Nothing routes a request through the policy or release
part any more, so it is dead by callers. `funding_v2` is different: it is still wired into the
PostgreSQL reserve and settlement transactions for accounts that carry a funding head, so removing
it changes a money transaction and needs the real-PostgreSQL matrices, not a local build.

The tables themselves (`account_policy_bindings`, `funding_buckets`, `funding_lots_v2`,
`pricing_*`) stay until an explicit drop migration, per the repository's expand-only rule. Until
then, nothing may start reading them again.
