# Multi-provider and model pricing policy handoff

Status: product requirements agreed; implementation has not started.

Last product discussion: 2026-07-29. Repository facts were inspected against the then-current
`origin/master`; this is an active trunk, so revalidate implementation references before editing.

This document records the intended replacement for the current scalar account-pricing model. It is
not a description of behavior already deployed. Until the feature is implemented, `PRICING.md` and
`CONTROL_API.md` describe the live contracts.

## Product objective

Support pricing rules at both provider and exact-model scope for:

- one global B2C policy;
- a separate, private policy for every B2B client;
- Anthropic and OpenAI today, without hard-coding the design to only those two providers;
- explicit model admission, so a model without administrative approval and an applicable price rule
  is rejected rather than receiving an accidental fallback price.

The first release supports the existing B2C discount track and static percentage discounts. Literal
fixed token tariffs are deliberately deferred, but the domain and engine boundary must leave a clean
model-only extension point for them.

## Terms

| Term | Meaning |
|---|---|
| Provider | A billing namespace such as `anthropic` or `openai`. |
| Canonical model | The one pricing identity to which public aliases resolve. |
| Provider rule | Default rule for every enabled model in one provider. |
| Model rule | Exact canonical-model override within a provider. |
| Track | The current progressive B2C tier plus any applicable referral floor. |
| Static discount | An administrator-set 0-95% discount from the complete official provider cost. |
| Fixed tariff | A future literal per-token/per-feature customer tariff for one exact model. |
| Enabled model | A model known to official metering and explicitly approved for commercial use. |

`discountPercent` means the percentage the customer does not pay. For example, a 60% discount means
the customer pays 40% of official cost. All persisted money and rate calculations remain integer
nanoUSD/basis-point operations; browser and JavaScript `number` values are never money authority.

## Settled rule matrix

| Policy owner and scope | Initial release modes | Future mode |
|---|---|---|
| Global B2C provider | `track`, static `discount` | None; provider-wide fixed tariffs are forbidden. |
| Global B2C model | `track`, static `discount` | Exact-model `fixed` tariff. |
| Individual B2B provider | Static `discount` | None; provider-wide fixed tariffs are forbidden. |
| Individual B2B model | Static `discount` | Exact-model `fixed` tariff. |

Discounts are integers from 0% through 95%, inclusive. A rule never stacks with another rule.

## Rule resolution and access

For an authenticated account and requested model:

1. Take the provider from the fixed API/provider plane that accepted the request, then resolve its
   canonical model. Never infer provider from model text or optional client headers.
2. Reject the request if the model is unsupported or is not administratively enabled.
3. Select the account's B2C policy or that exact B2B client's policy.
4. Use an exact model rule when one exists.
5. Otherwise use the provider rule when one exists.
6. Otherwise reject the request.

In compact form:

```text
exact model rule -> provider rule -> reject
```

An exact model rule replaces the provider rule; the two are never combined. A client may have exact
model rules without a provider rule. In that case only those exact models work and sibling models are
rejected.

A provider rule grants access to every enabled model under that provider. There is intentionally no
per-client model-block rule beneath a provider rule. If a B2B client must not receive all provider
models, configure only exact model rules and omit the provider rule.

Globally disabling a model makes it unavailable to every account, including accounts with an exact
model rule. Model enablement is an admission gate evaluated before account rule resolution.

When an administrator enables a new model, every B2C/B2B policy with a rule for that provider gains
access immediately. Policies containing only exact model rules do not gain the new model.

If an administrator deletes a provider rule while exact model rules remain, the exact models continue
to work and all other models in that provider become unavailable.

Failures caused by an unknown, disabled, or unpriced model should use the provider's normal
model-not-found response shape. They must not be reported as low balance or an internal billing error.

## Model catalog and aliases

Model support and pricing permission are separate gates:

1. The engine must know the official tariff and metering behavior for the model.
2. An administrator must enable the canonical model in the commercial catalog.
3. The applicable B2C/B2B policy must resolve to a provider or model rule.

A newly discovered upstream model remains unavailable until the first two gates are complete. A
provider rule must not silently authorize a model that has not been explicitly enabled.

The runtime-supported catalog and the commercially enabled catalog have different authorities:

- engine code/config owns provider capability, canonical aliases, and audited official metering;
- commerce PostgreSQL owns the administrator-controlled enabled state;
- the engine boundary needs a versioned, durable way to receive the enabled catalog alongside or
  before account policies, so both provider API planes enforce the same generation.

Enabling a model commercially is valid only after the running provider plane supports and meters it.
The implementation must define atomic propagation and readiness semantics; an admin write must not
advertise success while one serving process still rejects or misprices that generation.

Public aliases resolve before pricing. `gpt-5.6` is retained as a convenience alias for
`gpt-5.6-sol`; both names have one canonical pricing identity and one configurable rule. The alias
must never appear as an independently priced model or create a cheaper route to the same upstream
model.

## B2C behavior

### Initial state

All enabled B2C providers and models start on the existing B2C track. This includes Anthropic and
OpenAI. The current tier ladder, advancement thresholds, and rolling retention requirements remain
owned by `PRICING.md` and `B2C_PRICING_TIERS`; do not duplicate their numeric values into the new
policy implementation.

The B2C rules are global, but the effective track price is still account-specific. A `track` rule
selects each account's current tier/referral multiplier; it does not contain one global percentage.
Tier or referral-floor changes must produce a new effective account-policy generation and follow the
same durable synchronization path as an administrative pricing edit.

### Track eligibility

| Effective B2C rule | Uses tier/referral price | Counts toward 30-day tier retention | Referral commission eligible |
|---|---:|---:|---:|
| `track` | Yes | Yes | Yes |
| Static `discount` | No | No | No |
| Future model `fixed` | No | No | No |

A model-specific static discount can therefore remove one model from an otherwise provider-wide
track. Conversely, an exact `track` rule can put one model on the track even when its provider uses a
static discount.

The referral floor affects only requests whose resolved rule is `track`. Static-discount and future
fixed-tariff charges must not generate referral commission.

Every charge, including a non-track charge, must still be persisted in chronological financial
history and participate in free-first funding allocation, balance/spend reporting, refunds, and
audit. Track and commission eligibility are separate immutable flags; exclusion from those programs
must never mean dropping the charge or zeroing its real-funded amount.

Confirmed paid top-ups continue to advance the B2C tier regardless of which provider or pricing mode
later consumes the balance. For example, top-ups spent only on static-discount OpenAI traffic can
still unlock a better Anthropic track tier. Only track-eligible usage retains that tier during the
rolling 30-day window.

The current top-up advancement model is not being redesigned by this project.

## B2B behavior

Every B2B client owns a separate pricing policy. B2B clients never share a provider rule, model rule,
or mutable policy object, even when the configured percentages happen to be equal. There is no global
B2B fallback policy.

A B2B policy can contain:

- one discount for all enabled Anthropic models;
- one discount for all enabled OpenAI models;
- exact model discounts that override a provider discount;
- exact model discounts without any provider rule.

Creation of a B2B invitation requires at least one provider or exact-model rule. The invitation must
carry that prospective client's complete independent policy, and redemption must copy it atomically
to the new B2B client. Later edits affect only that client.

Every existing B2B client must also retain at least one rule. Reject an administrative edit that
would leave its policy empty.

Existing B2B clients currently have one scalar discount. At migration, copy that discount only to an
Anthropic provider rule. Do not create an OpenAI provider rule for them. Existing B2B clients receive
OpenAI access only after an administrator adds an OpenAI provider or exact-model rule.

Changing a rule has no billing-period delay, but the durable engine acknowledgement is the effect
point. The admin surface must show `pending` until the engine accepts the new generation. After that,
new requests use it immediately. A request already admitted under the previous rule finishes and
settles under that snapped rule; pricing must not change halfway through a request. Initial account
provisioning must install and acknowledge its complete policy before issuing a usable key.

## Official price modifiers

Percentage discounts apply to the complete effective official provider cost, not only base input and
output rates. The engine must first calculate official cost with all applicable model and feature
rules, then apply the selected customer discount.

This includes, where supported:

- input and output tokens;
- prompt-cache reads and writes;
- web-search request charges;
- effective-dated official tariff changes;
- Anthropic Fast Mode;
- OpenAI long-context pricing;
- Anthropic US inference geography.

For a static 60% discount, a premium official request still charges 40% of that premium official
cost. Provider/model discounts do not suppress provider feature premiums.

## `inference_geo: "us"`

Anthropic's first-party Messages API supports US-only inference for Claude 4.6 and later models. A
request can specify:

```json
{
  "inference_geo": "us"
}
```

`global` is the default. `us` requests that inference remain within the United States and applies an
official 1.1x multiplier to input, output, cache-read, and cache-write token prices. The separate web
search fee is not multiplied. Unsupported earlier models reject the option upstream.

The current source already implements this modifier:

- `crates/forward/src/proxy.rs` detects a requested US geography and reserves against 1.1x token
  rates;
- `crates/forward/src/meter.rs` reads the authoritative response usage geography for SSE and JSON,
  applies 1.1x to token cost buckets, leaves web search unchanged, and records the geography;
- the account multiplier is applied after the premium official cost is calculated.

The remediation entered history in commit `dcd6459`. It is an ancestor of the baseline used for this
handoff. The normal source path is mathematically correct, but no focused automated regression test
currently proves request/reservation/response mismatch cases. A live production SHA was not
independently verified during the product discussion. Add direct tests as part of implementation;
do not create a separate customer policy mode for inference geography.

## Deferred literal fixed tariffs

Literal fixed tariffs are not part of the initial release. Do not expose an incomplete fixed-tariff
form or allow a partially specified tariff to fall back to changing official rates.

The design must nevertheless allow a later exact-model rule that computes customer charge from a
complete literal tariff while retaining official provider cost separately for margin, capacity, and
audit reporting.

Settled future constraints:

- fixed tariffs are exact-model only;
- a provider can never have a literal fixed tariff because its models have different costs;
- fixed-tariff usage is outside B2C tier retention, referral discounts, and referral commission;
- Fast Mode and long-context variants use the fixed schedule defined for the qualifying model;
- fixed tariff changes apply to new requests while in-flight requests retain their snapped version.

Provider-specific fixed fields are deliberately not finalized. A later product decision must define
complete prices for every billable token/cache/tool bucket and every supported premium variant.

## Current implementation gap

Today customer pricing is scalar end to end:

```text
commerce customer_profiles.multiplier_bp
  -> engine_pricing_jobs
  -> worker POST /admin/account/{id}/pricing
  -> engine accounts.mult_bp
  -> every model on that account
```

Important current facts:

- commerce classifies B2C/B2B; the Rust engine currently sees only an account multiplier;
- B2C tiers and B2B invite/manual pricing live in commerce PostgreSQL;
- `business_invites`, `customer_profiles`, `engine_accounts`, and `engine_pricing_jobs` each carry a
  single multiplier;
- the durable pricing worker synchronizes one scalar multiplier;
- both Anthropic and OpenAI billing apply that same account multiplier after model-specific official
  metering;
- engine ledger rows expose an optional model, but commerce currently discards it when creating
  `pricing_usage_events`;
- the charge-ledger contract has no provider or immutable pricing-policy attribution;
- the customer dashboard and admin panel present one account discount and convert balance into one
  universal "official API value", which is invalid once rates differ by provider/model.

The existing B2C referral "fixed partner rate" is actually a floor over the progressive tier:
`min(tier multiplier, referral multiplier)`. It must not be reused as the future literal fixed-tariff
concept.

## Conceptual target model

The exact schema is an implementation decision, but the domain needs these concepts:

```text
ModelCatalogEntry
  provider
  canonical model
  aliases
  enabled state
  official-metering identity

PricingPolicy
  owner: global B2C or one B2B client
  version

EffectiveAccountPolicy
  enabled catalog generation
  account's current track multiplier
  materialized global-B2C or private-B2B rules

PricingRule
  scope: provider or canonical model
  mode: track or discount (fixed reserved for a later model-only extension)
  discount basis points when applicable
```

Commerce remains the policy authority. The engine should not need a B2C/B2B branch; commerce sends a
complete account policy and the engine enforces that account's resolved rules.

Policy updates should be atomic full-policy replacements with a monotonic version. Avoid one network
operation per model, which could expose a temporarily mixed generation.

At request admission the engine must resolve and snapshot at least:

- provider;
- requested model and canonical pricing model;
- selected provider/model rule;
- pricing mode and effective discount;
- policy version;
- enabled-catalog generation;
- official tariff epoch or admission pricing timestamp;
- B2C track/referral eligibility.

Reservation and settlement must use the same snapshot. Immutable usage/ledger attribution should
record enough of it that later policy edits cannot reinterpret historical charges or retention and
commission eligibility.

## Expected component impact

| Area | Expected responsibility |
|---|---|
| `crates/metering` | Stable canonical model/alias identities beside official cost schedules. |
| `crates/registry` | Versioned account policy persistence, atomic replacement/read, immutable settlement attribution, PostgreSQL migration, and required SQLite/import parity. |
| `crates/forward` | Resolve policy before reserve; carry one snapshot through Anthropic and OpenAI settlement; emit provider-shaped public errors for missing rules. |
| `crates/server` | Versioned Control API and validation for enabled catalogs and full account pricing policies. |
| `packages/contracts` | Provider/model catalog, policy, ledger attribution, invite, admin, and account-view schemas. |
| `packages/engine-client` | Atomic policy update/read methods and expanded ledger parsing. |
| `packages/db` | B2C provider/model policy authority, independent B2B/invite policies, versioned durable jobs, attribution and eligibility persistence. |
| `apps/worker` | Consume attributed ledger events and synchronize complete policy versions. |
| `apps/api` | B2B invite/redemption/edit APIs, B2C policy administration, provisioning, and customer projections. |
| `crates/server/src/admin-panel.html` | Provider defaults, expandable model overrides, B2B policy editing, effective-price/access preview. |
| `apps/web` | Provider/model pricing display; remove scalar-only value claims. |
| Sales bounded context | Exclude non-track events from referral commission and present correct eligibility. |

No pricing change is expected in `crates/pool`, payment-provider integrations, or Caddy routing.

Direct engine/service accounts, including accounts not represented by commerce B2C/B2B profiles,
are a compatibility surface. Strict account-policy rejection must not disable them accidentally.
Before cutover, choose and migrate an explicit service-account policy owner or retain a deliberate
legacy scalar policy for that account class. OpenKeys is also a separate product/account lifecycle and
must be assessed rather than assumed to inherit commerce B2C rules.

## Compatibility and rollout constraints

Persisted accounts, pending jobs, blue-green engine slots, and immutable financial history require a
compatibility rollout rather than an atomic code switch.

1. Deliver additive commerce and engine schema expansions first, following the repository's
   migration-first rules.
2. Deploy engine policy support with legacy scalar behavior preserved while no policy is present.
3. Add versioned Control API and commerce synchronization support.
4. Backfill global B2C Anthropic and OpenAI provider rules to `track`, without redundant exact-model
   rows; each B2C account keeps its own current track multiplier.
5. Backfill each existing B2B scalar discount to that client's Anthropic provider rule only.
6. Convert every unconsumed B2B invitation's scalar discount to an independent Anthropic-only
   policy, or deliberately invalidate it before dependent code ships.
7. Migrate direct service/OpenKeys account behavior explicitly.
8. Translate or drain pending scalar pricing jobs so they cannot overwrite a newer policy.
9. Synchronize and verify the enabled catalog and complete account policies before enabling strict
   missing-rule rejection.
10. Enable admin/customer surfaces and then make the versioned policy path authoritative.
11. Remove scalar-only contracts only in a later contract release.

Historical commerce events cannot be reconstructed reliably by provider/model because commerce did
not persist those fields. All pre-cutover B2C charge events were produced under the legacy track and
can be marked legacy track-eligible, but their provider/model must remain unknown. Preserve existing
tier-window aggregates and do not guess provider/model classifications from current rules.

## Required behavioral tests

At minimum, implementation must prove:

- exact model rule overrides provider rule without stacking;
- provider rule grants every enabled model and a newly enabled model immediately inherits it;
- exact-only policy grants only those models;
- missing/disabled model and missing price rule are rejected with provider-shaped errors;
- discount boundaries accept 0% and 95% and reject values outside that range;
- B2C `track` events count toward retention and referral commission;
- B2C static-discount events count toward neither;
- non-track charges still affect financial history, free-first funding, refunds, and spend totals;
- static-discount usage does not receive a referral floor;
- paid top-ups used on static-discount traffic still advance the B2C tier;
- every B2B client is isolated from every other B2B policy;
- invitation and existing-client edits reject an empty B2B policy;
- a provider rule cannot be combined with a per-client model block;
- B2B invitation redemption atomically copies its complete independent policy;
- existing B2B scalar discount backfills to Anthropic only;
- unconsumed legacy invitations are converted or invalidated safely;
- `gpt-5.6` and `gpt-5.6-sol` resolve to one policy and one price;
- official effective dates, cache buckets, web search, Fast Mode, long context, and US geography are
  included before percentage discount;
- policy change during an in-flight request does not change that request's settlement;
- retrying or superseding a durable policy job cannot install an older policy generation;
- engine PostgreSQL/SQLite compatibility and commerce integration migrations preserve existing
  accounts and financial history.

## Remaining product decisions

The following points were not settled and should be confirmed before implementation details are
documented as a final contract:

1. Whether a provider has its own enabled/disabled lifecycle and, if it does, whether disabling it
   preserves saved rules for later re-enable or deletes them.
   Recommended: preserve rules but reject traffic while disabled.
2. How the current `$10 of official usage` B2C signup promise works after one balance can buy different
   official value by provider/model. Options include making bonus funds track-only or changing the
   public promise to a fixed platform-balance amount.
3. The policy owner and migration behavior for direct engine/service and OpenKeys accounts.
4. The exact future literal fixed-tariff fields and premium schedules. This is intentionally deferred
   and does not block the initial release.
5. The final admin/customer presentation for effective provider/model prices and inaccessible models.

## Primary references

- `PRICING.md`
- `CONTROL_API.md`
- `COMMERCIAL_BACKEND.md`
- `PANEL.md`
- `packages/db/src/schema.ts`
- `packages/db/src/pricing.ts`
- `packages/contracts/src/index.ts`
- `packages/engine-client/src/index.ts`
- `apps/worker/src/pricing-worker.service.ts`
- `crates/metering/src/lib.rs`
- `crates/metering/src/codex.rs`
- `crates/forward/src/proxy.rs`
- `crates/forward/src/meter.rs`
- `crates/forward/src/codex/billing.rs`
- `crates/registry/src/lib.rs`
- `crates/registry/src/pg.rs`
- `crates/server/src/admin.rs`
- `crates/server/src/http.rs`
