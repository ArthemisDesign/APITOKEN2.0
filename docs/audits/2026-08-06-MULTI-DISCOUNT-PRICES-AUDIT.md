# Multi-discount prices update audit — 2026-08-06

Auditor: kimi k3 (independent code/docs audit, no production access).
Audited tree: `origin/master` at `9b537915` ("fix(pricing): show the release-authority
discounts on the customer dashboard post-cutover"). Scope: the entire
`docs/commerce/MULTI-DISCOUNT.md` contract — accepted decisions §1, economics §2–§5,
progressive removal §6, rollout machinery §7–§8, UI/API contracts §9, mandatory tests §10,
and the Definition of Done §11 as self-reported on 2026-08-06.

Method: static read of the engine (`crates/registry`, `crates/forward`, `crates/metering`),
commerce (`packages/db`, `packages/contracts`, `packages/engine-client`, `apps/api`,
`apps/worker`), storefronts (`apps/web`, `apps/admin`, `apps/openkeys`), the sales feed, and
the documentation set. No test suites were re-executed (the merge gate runs them on every
SHA); no production databases were touched — the DoD's production claims are taken from the
contract document, not re-verified here.

## Executive verdict

The core release-v2 economics are solid and match the contract: resolution order, basis-point
math, per-class policy validation, OpenKeys 1:1, service `meter_only`, bonus-first funding,
and the referral paid-funded basis all check out in code. The Definition of Done
nevertheless overclaims: two live functional gaps (post-cutover B2C→B2B conversion is
unenforceable; the deferred welcome-bonus settlement also pays password signups), one
recurring exact-contract rounding violation at settlement (carried over from the 2026-08-05
audit), and two progressive-removal leftovers in admin/customer UI contradict §11 items 2,
14, and 17 as written.

## Findings

### 1. High — post-cutover B2C→B2B conversion has no enforcement path

Contract decisions 13/14 require conversion and every B2B policy save to be enforced per
account in both eras: the strict lane before the head, the append-only assignment extension
after it ("the same operations propagate through the append-only assignment extension").

In code, a post-cutover conversion is never propagated:

- `convertToBusiness` (`apps/api/src/admin-operations.service.ts:302`) only updates the
  legacy scalar, provisions the managed policy/binding
  (`packages/db/src/pricing.ts:1666` → `pricing-policy-write.ts:1705`), and enqueues a legacy
  pricing job. It never calls `syncPricingReleasePolicyOverrideV2`, and no `apps/worker`
  sweep invokes the extension lane either.
- A later policy save cannot repair it: `syncPricingReleasePolicyOverrideV2` throws
  `assignment_conflict` when the base assignment class is not already `b2b`
  (`packages/db/src/pricing-provisioning-v2.ts:821`); `apps/api/src/admin.service.ts:726`
  catches that into a non-fatal response field, so the save "succeeds" without enforcement.
- Key issuance breaks as well: `ensurePricingReleaseProvisioningV2` throws
  `assignment_conflict` on the class mismatch for both base-covered accounts
  (`pricing-provisioning-v2.ts:709`) and extension-covered ones (`:733`), surfaced to the
  customer as a misleading 409 "pricing release provisioning is still pending"
  (`apps/api/src/account.service.ts:542`).

Net effect: a customer converted after the cutover keeps being billed under the B2C global
5000 bps base assignment on the release-v2 data plane and cannot issue new keys.
`docs/commerce/PRICING.md:93-103` overpromises ("Conversion … enforced … post-cutover … via
the assignment extension"). DoD item 17 ("holds") is inaccurate for post-cutover
conversions; the engine-side extension machinery could express a class-changing extension —
the commerce orchestration simply does not exist.

### 2. High — password signups receive the `$5` welcome bonus via the deferred path

Contract §5 and `docs/commerce/PRICING.md:42` state that password accounts do not receive
the bonus. The deferred settlement introduced by `462d78f8` ("grant the welcome bonus when
the engine account turns active asynchronously") does not distinguish them:

- `register()` records an unflagged antifraud profile for password users
  (`apps/api/src/auth.service.ts:116`, `:396`).
- The deferred settlement in `AccountService.ensureEngineAccount`
  (`apps/api/src/account.service.ts:91-101`) fires for any `b2c` profile that is ungranted
  and unflagged; neither `settleSignupBonus` (`apps/api/src/signup-bonus.ts:40`) nor
  `claimSignupBonus` (`packages/db/src/antifraud.ts:122`) checks for an OAuth identity.
- First account access (dashboard, keys) by a password user on an allowlisted mail domain
  therefore claims and credits exactly `5_000_000_000` nano.

Existing tests cover registration and the *recovery* branch
(`apps/api/src/account.service.test.ts:189`) but never the deferred-active branch for a
password user. Impact scales with password registration being open in production; the code
path itself is live regardless of configuration.

### 3. Medium (recurring from 2026-08-05) — settlement still rounds half-up; the contract says floor

The previous audit's "final customer debit violates the floor contract" remains open.
Reserve-time release-v2 holds correctly floor
(`crates/registry/src/pricing/release_v2.rs:654-658`), but final settlement on all three
providers still uses `metering::apply_multiplier`, which adds 5000 before division
(half-up; `crates/metering/src/lib.rs:471`, asserted by the test at `:1299`):

- Gemini: `crates/forward/src/gemini/billing.rs:979` (via `settled_charge_or_hold`);
- Codex/OpenAI: `crates/forward/src/codex/billing.rs:1185`, `:1323`;
- Anthropic: `crates/forward/src/meter.rs:516`.

The registry trusts the adapter-provided `actual_nano` at settlement
(`crates/registry/src/pg.rs:645`). The deviation is at most one nanoUSD per request in the
customer-overcharged direction, but it is an exact violation of
`charged_nano = floor(official_nano * payable_multiplier_bp / 10000)` and contradicts DoD
item 16 ("any deviation that bills a B2C customer off-policy is fixed"). The required
remediation is unchanged: one checked floor helper shared by reserve and all v2 settlement
paths; legacy snapshot replay (`crates/registry/src/pricing/policy.rs:289`) legitimately
keeps half-up as immutable history.

### 4. Low — admin still offers the "progressive tariff" control

`apps/admin/src/app/business/policy-editor.tsx:279` renders a `прогрессивный тариф`
(`track`) option whenever `allowTrack` is set, and
`apps/admin/src/app/pricing/page.tsx:95` sets it on the Global B2C policy page, whose
subtitle (`:281`) still reads "track и точные static overrides". §9 requires the admin
editor to not offer track/tier controls. Post-cutover the save is refused with
`release_cycle_required`, so the control is unsavable — but its presence violates §6 and
DoD item 14.

### 5. Low — customer dashboard still carries progressive copy

`apps/web/src/app/dashboard/sections/credits.tsx:61` tells the customer "Progressive and
fixed-discount rules can coexist", and `usage.tsx:501-503` keeps a live
`pricingMode === "track"` label branch rendering `copy.progressive`.
`docs/commerce/PRICING.md:216-224` claims no reader consumes progressive semantics; these
strings and branches are exactly such readers. Presentation-level, no billing effect.

### 6. Contract-clarity notes

- §9 ("the admin pricing editor must edit the global B2C default") vs shipped behavior
  (post-cutover global saves refused, engine hard-pins the B2C global rule to 5000 bps at
  `crates/registry/src/pricing/release_v2.rs:912-917`). `docs/commerce/PRICING.md:30-33`
  documents the refusal; the master contract does not. Amend §9 or build the release-cycle
  tooling.
- The literal rule-source enum `global_default | provider_override | model_override` exists
  only in the two documents; the dashboard renders per-provider/per-rule discount summaries
  instead. Intent largely met, contract literal not implemented.
- OpenKeys "every provider and model the engine can price": the pricing authority spans
  exactly `anthropic | openai | google`
  (`packages/db/src/pricing-stage5-materializer-v2.ts:499`); the KIMI/GLM engine lanes are
  outside the capability/release authority. The 2026-08-05 audit's "decision required"
  remains formally open.

## Verified conformant

- B2C resolution: exact model rule → provider rule → global → fail closed
  (`crates/registry/src/pricing/postgres.rs:4652-4681`);
  `payable_multiplier_bp = 10000 - discount_bps`, B2C global exactly 5000 bps, OpenKeys
  `discount_bps = 0`, no B2B global inheritance, service `meter_only` without a funding
  generation — all enforced engine-side (`release_v2.rs:884-967`).
- OpenKeys: runtime-following admission (`postgres.rs:4587`), master/scoped switches still
  fence, pricing-override fields rejected at every caller boundary
  (`apps/openkeys/src/lib/openkeys-pricing.ts:31-53`), DB `CHECK mult_bp = 10000` for the
  `official_1_to_1` contract.
- Service: the resolver skips every product gate except the master switch, charged hold is
  zero, no 402 path (`postgres.rs:4554-4561`, `release_v2.rs:662-667`).
- Funding: bonus-first ordering with a stored allocation-order invariant
  (`crates/registry/src/funding_v2.rs:283`, `:520`); welcome bonus exactly `$5.000000000`
  with idempotent `signup-bonus:<user-id>`; historical `$4` rows keep their NULL-amount
  nominal (`packages/db/src/antifraud.ts:158-175`).
- B2B migration scope: scalar converts into the single `provider:anthropic` rule only; no
  OpenAI/Gemini grants (`packages/db/src/pricing-stage5-materializer-v2.ts:619-624`).
- Sales feed: commissions only on `paid_funded_nano > 0`; bonus-funded, B2B, OpenKeys and
  service rows are excluded (`packages/db/src/sales-feed.ts:173-193`).
- Integer-money invariant: no `f64` in any pricing/funding Rust path; TypeScript
  `Number(...)` hits in pricing code are versions and timestamps only.
- §10 mandatory tests: every bullet statically maps to existing test files (resolution and
  provider-override data in `crates/registry/src/pg/tests.rs`, conversion/strict chain in
  `packages/db/src/strict-chain.integration.test.ts` and
  `pricing-policy-write.integration.test.ts`, OpenKeys in
  `apps/openkeys/src/lib/openkeys-pricing.test.ts`, service in `release_v2.rs` tests,
  welcome bonus in `apps/api/src/signup-bonus.test.ts`, referral in
  `packages/db/src/sales-feed.integration.test.ts`, funding backfill in
  `packages/db/src/multi-discount-backfill.integration.test.ts`, provisioning race in
  `packages/db/src/pricing-provisioning-v2.integration.test.ts`, Stage 8/9 and recovery in
  `packages/db/src/multi-discount-stage8-evidence.integration.test.ts` and
  `pricing-release-activation-authority.ts` tests). Presence mapped, not re-executed.

## Recommended remediation order

1. Close the post-cutover conversion lane: chain `syncPricingReleasePolicyOverrideV2`
   (extended to class-changing extensions) into `convertToBusiness`, or refuse post-cutover
   conversions loudly until a new release cycle; align `docs/commerce/PRICING.md:93-103`
   with the chosen behavior.
2. Gate the deferred welcome-bonus settlement to OAuth-attributed profiles (or flag
   password profiles at registration) and add the missing deferred-path password test.
3. Introduce one checked floor helper in `crates/metering` and use it for every
   v2-multiplied settlement (Gemini, Codex, Anthropic); keep half-up only for immutable
   legacy replay.
4. Remove the admin `allowTrack` option and the stale global-page subtitle; remove the
   customer-facing progressive explainer and the track label branch.
5. Reconcile the docs: §9 global-edit requirement vs the post-cutover refusal; the
   rule-source enum literal; the OpenKeys "every provider" wording vs the three-provider
   authority.
