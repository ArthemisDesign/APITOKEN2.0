# @claude-api/sales-api

Multi-level sales/referral partner portal backend (NestJS 11 on Fastify). A separate bounded
context with its own PostgreSQL (`@claude-api/sales-db`); its only link to commerce is the HTTP
internal feed at `COMMERCE_BASE_URL` (`/v1/internal/sales/*`, header `x-api-key: SALES_CONTROL_KEY`).
It never imports `@claude-api/db` and never opens the commerce database.

## Run

```bash
pnpm install
SALES_DATABASE_URL=postgres://... pnpm --filter @claude-api/sales-db db:migrate
pnpm --filter @claude-api/sales-api dev     # dev on http://127.0.0.1:3100
pnpm --filter @claude-api/sales-api build && pnpm --filter @claude-api/sales-api test
```

Environment: see `.env.example` (every variable documented there). Money is integer nanoUSD,
serialized as decimal strings in JSON. Sessions: opaque token in HttpOnly `sales_session` cookie
(SameSite=Lax, Secure in production); only the SHA-256 hash is stored. Verification/reset tokens
are stored hashed; raw tokens exist only AES-256-GCM-encrypted in the email outbox payload and are
never logged.

## Background loops (in-process)

- **Sync** (`SYNC_INTERVAL_MS`): pulls attributions, usage events and topups from the commerce
  feed with per-feed cursors; usage events of referred users produce the multi-level commission
  chain in one transaction (idempotent). A 404 feed (commerce side not deployed yet) is logged once
  at debug and retried.
- **Email delivery** (`EMAIL_POLL_INTERVAL_MS`): claims outbox rows with FOR UPDATE SKIP LOCKED,
  decrypts the token, renders "APIToken Partners" emails, sends via SMTP or logs metadata in
  `log` mode.

## Endpoints (global prefix `/v1`)

| Method | Path | Auth | Purpose |
|---|---|---|---|
| POST | /v1/auth/register | — | Register partner (optional `inviteCode` sets parent); always pending until email verified |
| POST | /v1/auth/login | — | Login; pending partners get `{verificationRequired:true}` without a session |
| POST | /v1/auth/logout | session | Revoke session, clear cookie |
| GET | /v1/auth/me | session | Current partner |
| POST | /v1/auth/email/verify | — | Consume verify token → status active + session |
| POST | /v1/auth/email/resend | — | Re-queue verification email |
| POST | /v1/auth/password/forgot | — | Queue reset email |
| POST | /v1/auth/password/reset | — | Consume reset token, set password, revoke sessions |
| GET | /v1/partner/overview | session | Referral code/URL, commission bps, counts, earnings totals, last30d |
| GET | /v1/partner/referrals | session | Referred users (masked ids) with spend/earned |
| GET | /v1/partner/earnings?days=30 | session | Daily spend/earned series |
| GET | /v1/partner/team | session | Direct sub-partners with their earnings + my override |
| POST | /v1/partner/invites | session | Create sub-partner invite (optional commissionBps preset) |
| GET | /v1/partner/invites | session | List invites |
| GET | /v1/partner/payouts | session | List own payouts |
| POST | /v1/partner/payouts | session | Request payout (validated against available earnings) |
| PATCH | /v1/partner/settings | session | Update displayName / payout method+details |
| GET | /v1/admin/overview | x-sales-admin-key | Program totals |
| GET | /v1/admin/partners | x-sales-admin-key | Partners with aggregates + parent info |
| PATCH | /v1/admin/partners/:id | x-sales-admin-key | Change commission bps / status |
| GET | /v1/admin/payouts?status= | x-sales-admin-key | List payouts |
| POST | /v1/admin/payouts/:id/decision | x-sales-admin-key | approve / reject / paid |
| GET | /v1/health, /v1/live, /v1/ready | — | Health/liveness/readiness |
