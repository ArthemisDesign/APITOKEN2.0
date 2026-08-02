# @claude-api/sales-db

PostgreSQL layer for the sales/referral partner portal (its own database, env `SALES_DATABASE_URL`).
This bounded context is fully separate from the commerce database: it never imports
`@claude-api/db` and only learns about commerce users/spend through the HTTP internal feed
consumed by `apps/sales-api`.

## Contents

- `src/schema.ts` — drizzle schema (partners, sessions, auth tokens, rate limits, email outbox,
  invites, referred users, sync cursors, usage events, topups, commission entries, payouts, audit log).
- `migrations/` — hand-written initial migration `0000_sales_init.sql` + drizzle journal.
- `src/migrate.ts` — migration runner with a pg advisory lock (`node dist/migrate.js`).
- `src/client.ts` — `createSalesDatabase(connectionString)` (pg Pool + drizzle).
- `src/secrets.ts` — AES-256-GCM encrypt/decrypt for raw auth tokens stored in the outbox payload
  (`SALES_TOKEN_ENCRYPTION_KEY`, 32-byte base64url).
- Typed repositories: `auth.ts`, `outbox.ts`, `referrals.ts`, `commissions.ts`, `payouts.ts`,
  `invites.ts`, `admin.ts`.

## Money

All money is integer nanoUSD carried as `bigint` (drizzle `bigint` mode `"bigint"`); SQL aggregates
are cast `::text` and parsed with `BigInt()`. Never JS `number` for amounts.

## Commission model

`computeCommissionChain(partnersChain, amountNano)` (pure, unit-tested):
level 0 = `amount * commission_bps / 10000` (integer floor) for the direct referrer; every next
level = previous level's amount `* sub_commission_bps / 10000` of that ancestor. The walk stops at
the first suspended partner (no entry, chain ends), at amount 0, or after level 10.
`recordUsageEvent` inserts the usage event and its full commission chain in one transaction,
idempotent via the unique `commerce_event_id`.

Migration `0015_paid_funded_commission_v2.sql` adds the dormant target authority in separate
`partner_usage_events_v2`, `pending_referral_usage_events_v2` and `commission_entries_v2` tables.
It has no pricing-mode field: eligibility is referred B2C plus positive exact
`paid_funded_nano`. A database trigger binds level 0 to the event's direct partner and each next
level to the previous partner's active parent, exact configured basis points and integer-floor
amount; usage and commission rows are immutable. The dual-schema consumer is a later checkpoint
after this migration's production watchdog is green.

## Commands

```bash
pnpm --filter @claude-api/sales-db build
pnpm --filter @claude-api/sales-db test          # pure commission-chain tests
SALES_DATABASE_URL=postgres://... pnpm --filter @claude-api/sales-db db:migrate
```
