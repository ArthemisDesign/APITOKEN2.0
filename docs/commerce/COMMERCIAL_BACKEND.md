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

The separate expand-only migration `packages/db/migrations/0028_pricing_stage5_evidence.sql`
adds empty Stage 5 run, typed-blocker and prepare/readback-ACK tables. It preserves the exact
validated inventories and plan needed to prove a later apply was rebuilt from stable engine and
OpenKeys scans; checks reject unequal scan digests and mismatched ACK readbacks. It neither seeds a
run nor enqueues or activates any release, and its consumer follows only after the migration SHA is
green in production.

Migration `packages/db/migrations/0029_pricing_release_two_phase_finalize.sql` removes the cyclic
requirement to know a live-money funding manifest before Stage 6 computes it. Stage 5 can persist an
immutable full-inventory plan with nullable balance funding and release identities. Guard triggers
allow only one-way funding finalization, reject source/policy or finalized-digest replacement and
require exact ready queue coverage before `prepared`. It performs no data backfill, starts no job,
does not move the release head and does not pause production writers. The dependent Stage 5/6
consumers are delivered only after this migration SHA is green.

The target Stage 6 worker consumer claims only explicitly staged `normalize_funding` target-plan jobs.
It performs two stable full cursor scans, excludes exact `meter_only` service inventory, then plans
and applies a bounded number of balance accounts per slice with durable leases and account-local
retries. Every POST is preceded by a fresh GET and uses only that response's source/target digests.
Parent confirmation requires a final repeat scan and exact queue coverage, then atomically freezes
assignment funding generations and the newly computed canonical funding manifest. Engine
target/recovery prepare+readback finalizes release digests afterward; this lane never moves the
release head. The pre-0029 orchestration remains dormant until replaced by that consumer.
Operational details and bounded env defaults are in `docs/commerce/MULTI_DISCOUNT_STAGE6.md`.

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

The same preflight/postflight boundary now covers release-v2 provisioning. While the global release
head is null, it is a no-op and preserves the pre-cutover path. Once a head exists, a key can be
returned only when the account is already in the immutable base release or after account-local
funding normalization, exact release-policy prepare/readback, atomic active/recovery assignment
extension and exact extension GET readback. A stale head retries the complete bounded chain; a
conflict or blocked funding plan fails closed. If the postflight check fails, the raw key is disabled
before it can reach the browser. This provisioning consumer cannot move the head.

Global activation is isolated in a separate durable worker lane. Strict contracts validate the
complete request, receipt and each typed rejection; `packages/engine-client` exposes the only
transport method; `packages/db/src/pricing-release-activation-jobs.ts` accepts only an explicitly
staged immutable job bound to persisted `passed=true`, zero-blocker Stage 8 evidence and prepared
target/recovery engine digests. The request is stored before network I/O, expired leases replay that
exact body, and confirmation atomically stores the complete validated ACK plus canonical
request/receipt result digest. Before the first delivery, the worker revalidates full engine and
OpenKeys inventories around a stable commerce/service snapshot, exact ownership/status and
post-cutover assignment-extension/funding authority. Subject identities remain hashed. Once a
request may have reached the engine, retry deliberately skips TTL and mutable-authority checks and
replays the immutable body so an applied CAS with a lost ACK can return `unchanged`. Forward
recovery derives its exact expected target head only from the durable cutover receipt. No API,
migration, startup hook or evidence collection automatically stages a job. New Stage 8 rows persist
both source-engine and service-inventory identities; staging rejects legacy rows where either
expand-only field is still `NULL`, and first delivery requires the fresh service digest to match.

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
GET       /admin/service-account-inventory
PUT       /admin/service-account-inventory/{service_id}
```

Policy writes are full CAS replacements over provider/model rules. Their responses include source
actor/reason/version, desired/applied target versions, durable delivery state and the latest error.
Service inventory is a separate admin-managed authority over
`service_account_inventory_v2`. A PUT is an exact per-service CAS: create requires null expected
version/digest, update requires the current pair, and an exact replay returns `unchanged`. The
operator supplies only stable service identity plus `purpose`/`responsible`; the API performs two
matching exhaustive engine pricing-inventory scans and copies the current engine status. It rejects
missing accounts, commerce mappings, OpenKeys handles, duplicate engine ownership and stale CAS,
then writes the row and audit evidence in one `SERIALIZABLE` commerce transaction. The mutation does
not create an engine account, policy, release or activation. Stage 5 still proves that the aggregate
commerce/OpenKeys/service inventories cover engine inventory exactly once, so an unregistered or
misclassified account remains a typed blocker rather than being inferred from its name.

Strict mutation body (unknown fields are rejected):

```json
{
  "expected_source_version": null,
  "expected_content_digest": null,
  "engine_account_id": "acct_...",
  "purpose": "internal workload description",
  "responsible": "platform owner",
  "reason": "operator audit reason"
}
```

GET and successful PUT return schema-v2 rows sorted by `service_id`, each with monotonic
`source_version` and canonical `content_digest`, plus one canonical `inventory_digest`. PUT also
returns `stored|unchanged` and the exact stable engine identity-inventory digest used for validation.

The resulting service inventory spans all runtime-capable models, carries authoritative
purpose/responsible evidence and materializes `billing_mode=meter_only`; it is not restricted by
product pricing rules and never depends on account balance.
An invitation may use either a complete policy or the legacy scalar compatibility input, never both;
policy-based invitations persist a neutral `10000` placeholder. Edit, resend and redemption preserve
independent exact snapshots, and redemption copies the selected invitation version into the new B2B
client policy before provisioning.

Stage 5 v2 consumes the already deployed engine/OpenKeys/service producers without changing a live
head. It requires a fresh dry-run digest and then performs one idempotent full-class apply:

```bash
pnpm --filter @claude-api/db pricing:stage5-v2 dry_run
pnpm --filter @claude-api/db pricing:stage5-v2 apply sha256:v2:<exact-plan-digest>
```

The command exhausts engine and OpenKeys cursors twice, snapshots commerce/service in
`REPEATABLE READ`, writes target/recovery skeletons in `SERIALIZABLE`, and records only exact
prepare+readback ACKs for dormant catalogs, switches and policies. It never creates a Stage 6 job,
prepares an engine release or moves capability/catalog/switch/release heads. Runtime credentials
come only from `DATABASE_URL`, `ENGINE_BASE_URL`, `ENGINE_CONTROL_KEY` and optional loopback
`OPENKEYS_INTERNAL_BASE_URL` / `OPENKEYS_CONTROL_KEY`; they are not command arguments or output.

Stage 6 is explicitly staged and observed by the exact Stage 5 plan digest:

```bash
pnpm --filter @claude-api/db pricing:stage6-v2 status sha256:v2:<exact-stage5-plan-digest>
pnpm --filter @claude-api/db pricing:stage6-v2 stage sha256:v2:<exact-stage5-plan-digest>
```

The CLI reads only `DATABASE_URL`; the deployed worker performs bounded account-local engine
normalization and target/recovery prepare. It never creates an activation job or advances a head.
The entrypoint must be invoked by the protected production control-plane, not through ad-hoc SSH;
until that operation records a confirmed production job, Stage 6 is not complete.

Stage 8 consumes the canonical schema-v2 engine report and emits one combined synchronization
identity. Generate `stage8-engine-evidence.json` first with the exact deployed `claude-api` binary
as described in `docs/ops/DEPLOYMENT.md`, then run:

```bash
pnpm --filter @claude-api/db pricing:stage8-evidence stage8-engine-evidence.json
```

The consumer preserves signed-i64 JSON money, independently recomputes the Rust evidence digest,
requires an engine source no older than 120 seconds, exhausts engine and OpenKeys twice and observes
commerce and service authority in one `SERIALIZABLE` snapshot. It binds exact prepared target/recovery
generations, semantic assignments, funding/release lineage, current inventories, runtime/shadow
evidence and the absence of pending or failed control jobs. A combined identity expires after 300
seconds. Existing target/recovery plans allow both passed and blocked reports to be persisted
immutably in `pricing_stage8_evidence_v2`; missing plans return `not_persisted`. Account and binding
subjects are emitted only as SHA-256 digests, blockers exit non-zero, and the command never seeds a
policy, retries a job or advances a head. Runtime credentials come only from `DATABASE_URL`,
required `ENGINE_CONTROL_KEY`, optional `ENGINE_BASE_URL`/`OPENKEYS_INTERNAL_BASE_URL`, and optional
dedicated `OPENKEYS_CONTROL_KEY` (otherwise OpenKeys uses the shared engine key).

The combined `sales_contract_digest` identifies the intended paid-funded commission schema but is
not deployed-sales-runtime evidence. Stage 9 separately requires the sales v2 consumer checkpoint.

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
