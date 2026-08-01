# Commercial backend

The commercial platform lives beside, but not inside, the Rust engine. It owns users, payment
provider state, webhook delivery state and the mapping from a commercial user to an engine account.
The Rust engine remains authoritative for API keys, live balances, reservations and usage charges.

## Workspace

```text
apps/api                 NestJS HTTP API (future browser/backend boundary)
apps/worker              Durable engine-credit, customer-pricing and SMTP email processor
packages/contracts       Shared validation and transport types
packages/db              PostgreSQL schema, migrations and repositories
packages/engine-client   Typed client for the Rust Control API
packages/payments        DigiSeller/Cryptomus adapters and normalized payment contracts
```

The applications are independently deployable. They share packages at build time, but neither
imports code from the Rust crates or opens the engine PostgreSQL database/SQLite migration snapshot.
Production deployment and rollback are documented in [`docs/ops/DEPLOYMENT.md`](../ops/DEPLOYMENT.md). The API is
immutable blue-green; the worker remains single-instance stop/start but runs from the exact same
immutable commerce release selected for the API.

## Local setup

Use Node.js 24 LTS and pnpm 9.

```bash
docker compose up -d commerce-postgres
pnpm install
cp apps/api/.env.example apps/api/.env
cp apps/worker/.env.example apps/worker/.env
# Without the production loopback Caddy listener, set local ENGINE_BASE_URL to the direct dev engine:
sed -i.bak 's#http://127.0.0.1:8790#http://127.0.0.1:8787#' apps/api/.env apps/worker/.env
pnpm db:migrate
pnpm build
pnpm dev:api
pnpm dev:worker
```

Production API and worker must instead use `ENGINE_BASE_URL=http://127.0.0.1:8790`; that stable,
loopback-only Caddy origin follows the healthy engine slot across blue-green cutovers.

Run the real PostgreSQL checkout/payment tests with:

```bash
TEST_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/commerce pnpm test:integration
```

Payment providers sit behind a provider-neutral adapter. Every adapter must verify
the provider event using its authoritative API and persist the webhook's globally unique event ID.
Only then may it create a payment and enqueue an engine credit. The worker uses the payment ID as a
stable, idempotent engine credit reference. Provider specifics are in `docs/commerce/DIGISELLER_INTEGRATION.md`
and `docs/commerce/CRYPTOMUS_INTEGRATION.md`.

Email/password authentication, authorization invariants and the future email/Google provider
boundaries are documented in `docs/commerce/AUTHENTICATION.md`.
Transactional email and self-hosted SMTP configuration are documented in `docs/commerce/EMAIL_INTEGRATION.md`.

B2C progressive tiers, B2B invitations/manual pricing, month-close behavior and the engine sync
pipeline are documented in `docs/commerce/PRICING.md`.

The multi-discount rollout adds a second, versioned synchronization lane beside the legacy scalar
multiplier lane. Commerce remains the policy authority: immutable catalog, provider-switch and
effective account-policy rows are staged into `engine_catalog_jobs`, `engine_switch_jobs` and
`engine_policy_jobs`. The worker claims them in catalog → switches → policy order, derives an exact
CAS expectation from the engine's active state, and stores the complete durable ACK before marking
the desired binding confirmed. Expired leases replay safely, including a lost ACK after the engine
commit; same-version/different-digest and malformed protocol responses are permanent failures.

This application checkpoint does not seed or activate production policies. A legacy scalar job is
drained only after its account has a non-null desired full-policy version and digest, so empty
version streams cannot alter current users. Provisioning becomes policy-before-key only after the
Stage 5 seed/backfill has created and delivered the relevant account policy; OpenKeys follows its
separate Stage 7 cutover.

## Authenticated client API

All private routes use the HttpOnly session cookie and derive the owner from that session. Engine
account IDs and the Control API key are never accepted from, or exposed to, the browser.

```text
GET    /v1/account                 live engine balance, reserved amount, spend, markup and status
GET    /v1/account/ledger?limit=50 live engine top-ups and charges (limit 1..1000)
GET    /v1/api-keys                masked keys with live status and per-key spend
POST   /v1/api-keys                {"label"?: "production"}; raw sk-pool key returned once
DELETE /v1/api-keys/{id}           disable an owned key by commercial UUID
```

`GET /v1/account` also returns the authenticated customer's safe pricing view. Commercial operator
routes use a separate `COMMERCIAL_ADMIN_KEY`; they create email-bound or copy-only B2B
invitations, revoke/rotate them, and change B2B pricing. That key is never a client session or an
engine Control API credential.

Engine provisioning is recoverable: the stable handle `user:<commercial UUID>` makes account
creation idempotent. API-key revocation uses the engine's non-secret `key_id`; PostgreSQL stores the
commercial UUID, engine `key_id`, and mask, never the usable key. A key issued by the engine is
immediately disabled as compensation if its commercial record cannot be committed.

New Google/GitHub B2C provisioning also sends one idempotent `$4.000000000` signup credit using
`signup-bonus:<commercial UUID>`. Because Starter clients pay 40% of official prices, the public
offer is stated as **$10 of usage at official API prices**. Password accounts are ineligible even
when their address is hosted by Gmail. Recovery derives eligibility from the stored OAuth identity
and repeats the same reference, so it cannot double-credit; B2B invitation provisioning skips it.

## Top-up money contract

Top-ups are not products and there is no fixed catalog. The user submits a JSON **string** containing
positive whole USD digits, for example `{"amountUsd":"37"}`. Values containing a decimal point,
sign, leading zero or a JSON number are invalid. PostgreSQL stores `amount_usd` as `bigint` and
enforces `amount_nano = amount_usd * 1_000_000_000`. Provider-formatted `"37.00"` is accepted only
when every fractional digit is zero.

The local checkout is authoritative for user, engine account and amount. A provider webhook cannot
change any of them. `paid_over` credits only the requested amount; underpayment never credits.

## Ownership rules

- PostgreSQL `commerce` is the source of truth for users, payments and webhook processing.
- The Rust Control API is the only allowed path to engine accounts and balances.
- Live balance, ledger, key status and per-key spend are read from the engine, not cached as money.
- Never copy the Control API key into the frontend or return it from an HTTP response.
- Store requested whole USD and engine nanoUSD as PostgreSQL `bigint`; never use floating point.
- A payment webhook may enqueue at most one credit; an engine credit reference may confirm once.
- The worker may retry indefinitely until the engine confirms the credit or an operator marks it dead.
- B2C tier progress comes only from authoritative engine `charge` ledger rows and is deduplicated by
  `(engine_account_id, ledger_entry_id)`.
- Legacy scalar pricing changes are persisted as durable jobs before the engine multiplier is
  updated. Once an account has a desired full policy, that scalar lane is audit-drained and only
  the monotonic versioned policy lane may advance its engine pricing state.
