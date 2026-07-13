# Customer pricing

Pricing is owned by the commercial PostgreSQL service and enforced by the Rust engine's account
multiplier. The engine ledger remains authoritative for usage; the commercial worker consumes
charge rows through an exact, idempotent cursor and never estimates spend from browser data.

## B2C progressive pricing

All thresholds are the customer's actual local balance spent during one UTC calendar month.
Promotions apply immediately. The achieved tier carries into the next month. If the customer does
not meet the retained tier's threshold in a month, month close moves them down exactly one tier.

| Tier | Client discount | Local monthly spend | Rounded official API usage shown to client |
|---|---:|---:|---:|
| Starter | 60% | $0 | $0 |
| Builder | 65% | $25 | $60+ |
| Pro | 70% | $75 | $200+ |
| Studio | 75% | $200 | $600+ |
| Scale | 80% | $500 | $1,800+ |

Money is stored as integer nanoUSD. The engine multiplier is the percentage the client pays:
Starter is `4000` (40%), Builder `3500`, Pro `3000`, Studio `2500`, and Scale `2000`.

The pricing worker paginates `GET /admin/account/{id}/ledger?after_id=...`, deduplicates by engine
account and ledger ID, updates the current month, and creates a durable pricing job when the tier
changes. A separate job step calls the engine pricing endpoint. Failed synchronization is retried;
PostgreSQL and the engine never need a distributed transaction.

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
- `PRICING_CLOSE_GRACE_MS` delays UTC month close to allow late ledger synchronization (default 1 hour).
- Existing users are backfilled as Starter and active engine accounts receive a durable sync job.
- Account responses expose a safe `pricing` view with tier, monthly progress, next threshold, and
  discount. B2B responses expose only manual pricing and the current discount.
