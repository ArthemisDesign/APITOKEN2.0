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

Migration `0024_team_override_controls.sql` is the migration-first expansion for individual Team
overrides. It adds a hard-bounded `team_override_max_bps` (0..2000) to partners/invites and an
optional `parent_override_bps` on the child/invite edge. A NULL ceiling is the rolling-deploy/default
20% ceiling, while existing NULL edges retain the deployed parent `sub_commission_bps` calculation.
Cross-row guards prevent a child rate or
delegated ceiling from exceeding the direct parent's ceiling and prevent lowering a ceiling below
an explicit dependent grant. The immutable v2 commission trigger accepts the new edge with the
same NULL fallback. This migration does not write an edge, change an existing commission, or enable
a consumer; application/API/UI use follows only after the exact migration SHA is production GREEN.

Migration `0015_paid_funded_commission_v2.sql` adds the dormant target authority in separate
`partner_usage_events_v2`, `pending_referral_usage_events_v2` and `commission_entries_v2` tables.
It has no pricing-mode field: eligibility is referred B2C plus positive exact
`paid_funded_nano`. A database trigger binds level 0 to the event's direct partner and each next
level to the previous partner's active parent, exact configured basis points and integer-floor
amount; usage and commission rows are immutable. The dual-schema consumer is a later checkpoint
after this migration's production watchdog is green.

Migration `0016_topups_v2_cursor.sql` widened the existing `sync_cursors` key check with
`topups_v2` before the consumer shipped. The live consumer owns this independent sequence cursor;
the legacy `topups` timestamp cursor remains stored and is never rewritten by v2 replay.

Migration `0017_payment_reversal_accounting.sql` is the migration-first checkpoint for exact
partner chargeback accounting. It reserves and seeds independent `topup_funding_lots` and
`payment_reversals` cursors. The former replays the commit-ordered `topups-v2` source from zero
without resetting the live analytics cursor. The schema
snapshots referred topups as immutable paid-funding lots, and provides append-only allocation
tables from each scalar/v2 usage row to those FIFO lots and from each commission entry to its exact
lot-funded slice. A reversal is unique by Commerce reversal, payment and lot; its adjustment ledger
accepts only the exact negative commission slice funded by that payment. Database guards enforce
source identity, causal paid-at/reversed-at order, per-user FIFO, lot/usage bounds, deterministic
integer rounding, SERIALIZABLE reversal writes, a deferred complete-adjustment-set check and
immutability. This commit creates no consumer, backfill, adjustment or payout
side effect; those ship only after the production migration is green.

Migration `0018_reversal_completeness_fence.sql` closes the zero-adjustment and late-allocation
holes before the consumer ships. Reversal creation, usage allocation, commission allocation and
adjustment insertion serialize on the immutable funding-lot row. A reversal transaction is always
`SERIALIZABLE` and has its own deferred completeness trigger, so zero inserted adjustments cannot
bypass validation. Deferred guards on later allocations require the same transaction to append all
exact negative slices when the lot is already reversed; otherwise the write fails closed. The
consumer also proves both Commerce source cursors caught up before reversal processing, because a
Sales-database constraint cannot see a Commerce row that has not crossed the HTTP boundary yet.

`src/reversal-accounting.ts` implements the consumer-side writer after both schema migrations are
green. It snapshots the independent commit-ordered funding-lot replay, allocates scalar/v2 usage by
causal FIFO, allocates every commission row with the schema's cumulative integer-floor rule, and
commits each reversal plus all exact negative entries and its source cursor in one `SERIALIZABLE`
transaction. Every immutable replay is compared field-for-field. Missing lots, incomplete funding
evidence and conflicts fail closed before cursor advance. The Sales sync loop additionally proves
fresh usage and funding-lot requests made after a non-empty reversal page both return no-advance
pages before admitting that reversal; their unrelated sequence values are never compared.

## Commands

```bash
pnpm --filter @claude-api/sales-db build
pnpm --filter @claude-api/sales-db test          # pure commission-chain tests
SALES_DATABASE_URL=postgres://... pnpm --filter @claude-api/sales-db db:migrate
```
