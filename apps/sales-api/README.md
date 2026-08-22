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

- **Sync** (`SYNC_INTERVAL_MS`): pulls attributions, usage events, commit-ordered `topups-v2` and
  payment reversals
  from the commerce feed with per-feed cursors; usage events of referred users produce the
  multi-level commission chain in one transaction (idempotent), while deposits are reporting-only
  and idempotent by payment id. The legacy topup cursor is retained but no longer advanced. A 404
  feed (commerce side not deployed yet) is logged once at debug and retried.
- **Email delivery** (`EMAIL_POLL_INTERVAL_MS`): claims outbox rows with FOR UPDATE SKIP LOCKED,
  decrypts the token, renders "APIToken Partners" emails, sends via SMTP or logs metadata in
  `log` mode.
- **Partner request effects** (`PARTNER_EFFECT_POLL_INTERVAL_MS`): claims approved B2B requests
  with a unique lease token, calls Commerce with the stable request operation reference, and only
  then moves the request to `applied`. Transport/5xx failures retry with bounded exponential delay;
  request/ownership/idempotency conflicts remain visible as terminal `apply_failed` decisions.
  Request responses expose `nextAttemptAt` and `terminal`; a terminally closed request no longer
  blocks a separately reviewed successor request for the same referral.

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
| POST | /v1/partner/requests/commission | session + Idempotency-Key | Request a higher platform commission with a reason |
| POST | /v1/partner/referrals/:userRef/b2b-requests | session + Idempotency-Key | Request B2B conversion/pricing for an owned referral |
| GET | /v1/partner/requests | session | Keyset-paginated own request history |
| GET | /v1/partner/payouts | session | List own payouts |
| PATCH | /v1/partner/settings | session | Update displayName / payout method+details |
| GET | /v1/admin/overview | x-sales-admin-key | Program totals |
| GET | /v1/admin/partners | x-sales-admin-key | Partners with aggregates + parent info |
| PATCH | /v1/admin/partners/:id | x-sales-admin-key | Change commission bps / status |
| GET | /v1/admin/requests | x-sales-admin-key | Keyset-paginated partner decision queue |
| GET | /v1/admin/requests/:id | x-sales-admin-key | Request, immutable terms, decision and effect state |
| POST | /v1/admin/requests/:id/decision | x-sales-admin-key + X-Admin-Actor | Approve/reject with a mandatory note |
| GET | /v1/admin/payouts?status= | x-sales-admin-key | List payouts |
| POST | /v1/admin/payouts/:id/decision | x-sales-admin-key | Reject a retained legacy manual payout; positive payouts use fenced batches |
| GET | /v1/health, /v1/live, /v1/ready | — | Health/liveness/readiness |
