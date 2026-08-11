# Customer pricing

Status: live entry point. The canonical customer-pricing and funding contract is
`docs/commerce/PRICING_MODEL.md`. This file intentionally does not carry a second policy
specification; model, price, commission and refund changes must update that contract and the
corresponding checklist in `docs/CHANGE_CHECKLISTS.md`.

## Current authority

- The engine account has one payable default, `accounts.mult_bp`, bounded to `0..10000`.
- Optional `account_provider_discounts` rows override that default for one canonical provider.
- Commerce persists the same desired default and provider rows, then delivers them through the
  fenced `engine_pricing_jobs` queue. A pricing edit is effective on the next authorization read;
  there is no catalog, policy binding, release head or activation sequence.
- B2C uses its stored scalar (normally 5000 today). B2B uses its negotiated default and optional
  provider overrides. OpenKeys is exactly 10000. Service traffic uses the explicit zero-payable
  scalar while retaining usage metering.
- Customer money is the engine account balance. Commerce's paid/free attribution is downstream
  commission/refund evidence and never a second admission balance. Pool-funded settlement
  shortfall and `admin-credit:*` do not become partner commission.
- The sales producer emits the scalar customer-funded basis. Historical policy-v1/release-v2
  forms remain consumer-readable only for expand-only replay (`docs/sales/SALES_PORTAL.md`).

The exact Control API shapes are in `docs/engine/CONTROL_API.md`; OpenKeys specifics are in
`docs/product/OPENKEYS.md`. Money remains integer nanoUSD throughout.

## Retired authority

The policy/catalog/switch/release/funding design was removed from every runtime path on
2026-08-09 after its two funding representations made funded accounts fail admission. Its schema
is immutable incident evidence, not a dormant fallback or rollback API. Do not execute historical
strict-chain/backfill instructions or restore their routes.

The exact 31 engine and 43 commerce table manifests, the retention boundary after
`2026-09-09 09:26:32 UTC`, rollback-release fence, dependency checks, backup requirements and
forward-only contraction order are authoritative in `docs/ops/PRICING_RETIREMENT.md`.
