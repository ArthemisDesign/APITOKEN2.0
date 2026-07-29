# Customer pricing

Pricing is owned by the commercial PostgreSQL service and enforced by the Rust engine's account
multiplier. Confirmed top-ups are authoritative for B2C tier advancement; idempotently consumed
engine charge-ledger rows are authoritative for usage and rolling retention spend.

## B2C progressive pricing

B2C advancement is based on cumulative confirmed top-ups, not usage spend or UTC calendar-month
charges. Promotions apply immediately when cumulative top-ups reach a threshold. Above Starter,
the customer retains the tier by spending at least its `holdUsd` during each rolling 30-day window.
Missing the hold moves the customer down exactly one tier and resets eligible cumulative progress to
the lower tier's threshold. Starter has no retention requirement.

### B2C signup usage

Each newly provisioned B2C account receives **$10 of usage at official API prices**. Starter charges
40% of official prices, so the engine receives an exact `$4.000000000` balance credit. The credit
uses the stable `signup-bonus:<commercial user UUID>` reference and is therefore safe to retry
without double-crediting. Invited B2B accounts do not receive this B2C offer.

| Tier | Client discount | Cumulative top-ups | 30-day hold (`holdUsd`) | Rounded official API usage shown to client |
|---|---:|---:|---:|---:|
| Starter | 60% | $0 | $0 | $0 |
| Builder | 62.5% | $100 | $50 | $267 |
| Pro | 65% | $250 | $125 | $714 |
| Studio | 67.5% | $500 | $250 | $1,538 |
| Scale | 70% | $1,000 | $500 | $3,333 |

The displayed official API usage milestone is calculated from the threshold and discount on the
same row: `cumulative top-up threshold / (1 - discount)`, then rounded for presentation. Billing
keeps exact integer nanoUSD values. For example, Scale charges 30% of official API prices, so the
`$1,000 / 0.30` milestone is displayed as `$3,333` of official API usage.

Money is stored as integer nanoUSD. Client contracts carry nanoUSD as decimal strings; browser code
must sum, compare, and format them with integer/BigInt logic, never through JavaScript `number`.
Requests are metered as official API spend first, then the engine applies the account multiplier to
determine the local balance charge. The multiplier is the percentage the client pays: Starter is
`4000` (40%), Builder `3750`, Pro `3500`, Studio `3250`, and Scale `3000`.

### Effective official model pricing

Claude Sonnet 5 uses Anthropic's **introductory official pricing** of **$2 input / $10 output per
1M tokens through 2026-08-31**. At `2026-09-01T00:00:00Z`, it returns to **$3 input / $15 output per
1M tokens**. The engine applies the current effective-dated official rate automatically before the
customer's account multiplier.

The pricing worker paginates `GET /admin/account/{id}/ledger?after_id=...`, deduplicates charge rows
by engine account and ledger ID, assigns exact spend to the active 30-day retention window, and
creates a durable pricing job when the tier changes. A separate job step calls the engine pricing
endpoint. Failed synchronization is retried; PostgreSQL and the engine never need a distributed
transaction.

## B2B pricing

B2B accounts bypass the progressive table. An operator creates a one-time, email-bound invitation
with an expiry and an integer discount percentage. Only the SHA-256 token hash is stored. A matching
registration consumes it atomically and provisions the engine account with the configured price.
Operators can later change that user's discount; the update is persisted locally and synchronized
to the engine through the same durable job path.

Administrative routes require `x-admin-key: <COMMERCIAL_ADMIN_KEY>`:

```text
POST  /v1/admin/business-invites
      {"email":"founder@example.com","discountPercent":85,"expiresInDays":7}

PATCH /v1/admin/business-users/{userId}/pricing
      {"discountPercent":87}
```

The invite URL is returned only from the create call. The registration endpoint accepts it as
`inviteToken`; it is not accepted on login. Unsafe requests still require the configured app origin.

## Operations

- `PRICING_POLL_MS` controls ledger and pricing-job polling (default 60 seconds).
- `PRICING_CLOSE_GRACE_MS` gates retention-window closure during the UTC month-close grace period (default 1 hour).
- Existing users are backfilled as Starter and active engine accounts receive a durable sync job.
- Editing the `B2C_PRICING_TIERS` ladder is picked up automatically: on pricing-worker start,
  `reconcileTierLadderMultipliers` re-derives every b2c profile's multiplier from its current tier
  (referral floors preserved) and pushes changes to the engine through the durable job path. B2B
  pricing is never touched by this reconciliation.
- Account responses expose a safe `pricing` view with tier, cumulative top-up progress, next
  threshold, rolling-window retention spend, and discount. B2B responses expose only manual pricing
  and the current discount.
