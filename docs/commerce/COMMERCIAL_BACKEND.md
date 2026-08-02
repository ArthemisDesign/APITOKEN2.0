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
packages/payments        Platega/Cryptomus adapters (DigiSeller registered but disabled) and normalized payment contracts
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
stable, idempotent engine credit reference. Provider specifics are in `docs/commerce/PLATEGA_INTEGRATION.md`
(default), `docs/commerce/CRYPTOMUS_INTEGRATION.md` and `docs/commerce/DIGISELLER_INTEGRATION.md` (disabled).

Email/password authentication, authorization invariants and the future email/Google provider
boundaries are documented in `docs/commerce/AUTHENTICATION.md`.
Transactional email and self-hosted SMTP configuration are documented in `docs/commerce/EMAIL_INTEGRATION.md`.

B2C global/provider/model discounts, B2B invitations/manual policies, OpenKeys/service boundaries
and the zero-downtime engine sync pipeline are documented in `docs/commerce/PRICING.md` and
`docs/commerce/MULTI-DISCOUNT.md`. Progressive tiers, retention and `track` are migration-only
legacy and must not receive new behavior.

The multi-discount rollout adds a second, versioned synchronization lane beside the legacy scalar
multiplier lane. Commerce remains the policy authority: immutable catalog, provider-switch and
effective account-policy rows are staged into `engine_catalog_jobs`, `engine_switch_jobs` and
`engine_policy_jobs`. The worker claims them in catalog → switches → policy order, derives an exact
CAS expectation from the engine's active state, and stores the complete durable ACK before marking
the desired binding confirmed. Expired leases replay safely, including a lost ACK after the engine
commit; same-version/different-digest and malformed protocol responses are permanent failures.

Commerce migration `packages/db/migrations/0026_pricing_release_expand.sql` is the dormant
migration-first foundation for the target Stage 5/6/8/9 flow. It adds versioned target policy and
rule documents, B2B invitation snapshots, service inventory, target/recovery plans, full-inventory
assignments, resumable funding/control jobs, Stage 8 evidence and activation receipts. Service
policies intentionally have no product catalog or product-switch pins because their access is
runtime-capability-gated and `meter_only`. The migration does not seed a policy, enqueue a job or
activate a release; dependent readers and writers are delivered only after this schema SHA has
green `deploy/migration` and `deploy/watchdog` in production.

The Stage 6 worker consumer claims only explicitly staged `normalize_funding` target-release jobs.
It performs two stable full cursor scans, excludes exact `meter_only` service inventory, then plans
and applies a bounded number of balance accounts per slice with durable leases and account-local
retries. Every POST is preceded by a fresh GET and uses only that response's source/target digests.
Parent confirmation requires a final repeat scan, exact queue coverage and the immutable target
funding manifest; this lane never moves the release head. Operational details and bounded env
defaults are in `docs/commerce/MULTI_DISCOUNT_STAGE6.md`.

This application checkpoint does not seed production policies or enable the engine's strict runtime.
A legacy scalar job is drained only after its account has a non-null desired full-policy version and
digest, so empty version streams cannot alter current users. Application provisioning is now
conditionally policy-before-key: when a managed Global B2C or B2B source policy exists, commerce
creates the binding, immutable effective version and durable job before a usable key, keeps the
engine account pending, and activates it only after the exact ACK. Accounts with no managed policy
authority retain the legacy path until Stage 5 creates that authority. OpenKeys follows its separate
Stage 7 cutover.

Key issuance checks exact desired/applied version and digest both before and after the remote issue.
If policy authority appears or changes in that race window, commerce disables the just-issued engine
key as compensation and never stores it as usable. A provider-switch update creates a new immutable
generation and rematerializes every existing managed binding against it, preserving unrelated
product/OpenKeys scopes while preventing stale switch lineage.

The commercial admin API exposes the complete managed surface:

```text
GET       /admin/pricing-catalog?product_id=...
PATCH     /admin/provider-switches
GET/PATCH /admin/pricing-policies/global-b2c
GET       /admin/business-users/{id}/pricing-policy
PATCH     /admin/business-users/{id}/pricing
GET/PATCH /admin/business-invites/{id}/pricing-policy
GET       /admin/service-policies
GET/PATCH /admin/service-policies/{id}?product_id=...
```

Policy writes are full CAS replacements over provider/model rules. Their responses include source
actor/reason/version, desired/applied target versions, durable delivery state and the latest error.
Service inventory spans all runtime-capable models, carries authoritative purpose/responsible
evidence and materializes `billing_mode=meter_only`; it is not restricted by product pricing rules
and never depends on account balance.
An invitation may use either a complete policy or the legacy scalar compatibility input, never both;
policy-based invitations persist a neutral `10000` placeholder. Edit, resend and redemption preserve
independent exact snapshots, and redemption copies the selected invitation version into the new B2B
client policy before provisioning.

Stage 8 adds a read-only synchronization report for the commerce side:

```bash
DATABASE_URL=postgresql://... pnpm --filter @claude-api/db pricing:stage8-evidence
```

The command observes one `REPEATABLE READ READ ONLY` snapshot and prints a canonical JSON report.
The target report verifies Anthropic/OpenAI/Gemini capability and catalogs, complete authoritative
classification, B2C/B2B/OpenKeys/service target policies, funding generation, desired/applied ACKs,
prepared target/recovery releases, stale generations and the absence of pending control jobs.
Account and binding subjects are emitted only as SHA-256 digests. A report with blockers is still
printed and exits non-zero. It never seeds policies, advances a head or retries a job. The old
reviewed Stage 5 assignment matrix is replaced by exact authoritative inventory coverage; inject
the DSN through protected environment rather than placing production credentials in shell history.

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

`GET /v1/account` also returns the authenticated customer's safe funding and desired/applied pricing
view, including provider/model availability without inferring provider from model names. Commercial
operator routes use a separate `COMMERCIAL_ADMIN_KEY`; they create email-bound or copy-only B2B
invitations, revoke/rotate them, and manage full B2B pricing policies. That key is never a client
session or an engine Control API credential.

Engine provisioning is recoverable: the stable handle `user:<commercial UUID>` makes account
creation idempotent. API-key revocation uses the engine's non-secret `key_id`; PostgreSQL stores the
commercial UUID, engine `key_id`, and mask, never the usable key. A key issued by the engine is
immediately disabled as compensation if its commercial record cannot be committed.

The target Google/GitHub B2C provisioning sends one idempotent `$5.000000000` signup credit using
`signup-bonus:<commercial UUID>`. It can fund every model allowed by the B2C policy. Password
accounts are ineligible even when their address is hosted by Gmail. Recovery derives eligibility
from the stored OAuth identity and repeats the same reference, so it cannot double-credit; B2B
invitation provisioning skips it. Existing `$4` grants remain unchanged.

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
- B2C usage rows are deduplicated by `(engine_account_id, ledger_entry_id)` and accepted only after
  exact local policy/admission/funding validation. Sales receives exact referred-B2C
  `paid_funded_nano`; commission eligibility is independent of pricing mode, and welcome-funded
  usage is excluded.
- Legacy scalar pricing changes are persisted as durable jobs before the engine multiplier is
  updated. Once an account has a desired full policy, that scalar lane is audit-drained and only
  the monotonic versioned policy lane may advance its engine pricing state.
