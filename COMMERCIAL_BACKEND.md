# Commercial backend

The commercial platform lives beside, but not inside, the Rust engine. It owns users, payment
provider state, webhook delivery state and the mapping from a commercial user to an engine account.
The Rust engine remains authoritative for API keys, live balances, reservations and usage charges.

## Workspace

```text
apps/api                 NestJS HTTP API (future browser/backend boundary)
apps/worker              Durable engine-credit processor
packages/contracts       Shared validation and transport types
packages/db              PostgreSQL schema, migrations and repositories
packages/engine-client   Typed client for the Rust Control API
packages/payments        DigiSeller/Cryptomus adapters and normalized payment contracts
```

The applications are independently deployable. They share packages at build time, but neither
imports code from the Rust crates or opens the engine SQLite database.

## Local setup

Use Node.js 24 LTS and pnpm 9.

```bash
docker compose up -d commerce-postgres
pnpm install
cp apps/api/.env.example apps/api/.env
cp apps/worker/.env.example apps/worker/.env
pnpm db:migrate
pnpm build
pnpm dev:api
pnpm dev:worker
```

Payment providers sit behind a provider-neutral adapter. Every adapter must verify
the provider event using its authoritative API and persist the webhook's globally unique event ID.
Only then may it create a payment and enqueue an engine credit. The worker uses the payment ID as a
stable, idempotent engine credit reference. Provider specifics are in `DIGISELLER_INTEGRATION.md`
and `CRYPTOMUS_INTEGRATION.md`.

## Ownership rules

- PostgreSQL `commerce` is the source of truth for users, payments and webhook processing.
- The Rust Control API is the only allowed path to engine accounts and balances.
- Never copy the Control API key into the frontend or return it from an HTTP response.
- Store provider amounts in integer minor units and engine amounts as PostgreSQL `bigint`.
- A payment webhook may enqueue at most one credit; an engine credit reference may confirm once.
- The worker may retry indefinitely until the engine confirms the credit or an operator marks it dead.
