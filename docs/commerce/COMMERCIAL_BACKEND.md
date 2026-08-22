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

## Frontend security headers

The two customer-facing Next.js apps send a Content-Security-Policy from their
`next.config.ts` (in addition to nosniff/referrer/permissions headers):

- `apps/web` (customer frontend): `default-src 'self'`; scripts allow `'unsafe-inline'`
  (Next 16 inline scripts + theme/referral/Metrika bootstrap) and `https://mc.yandex.ru`
  (Yandex Metrika loader); `connect-src` allows `https://backend.apitoken.sale`
  (the `NEXT_PUBLIC_BACKEND_URL` default) and `https://mc.yandex.ru` (Metrika beacon).
- `apps/sales-web` (partners portal): `default-src 'self'`; scripts allow `'unsafe-inline'`
  and `https://telegram.org` (Telegram Login Widget loader); `frame-src` allows
  `https://oauth.telegram.org` (the widget's embedded iframe).

A change that adds another external origin to either app (new analytics, widget, font or
API host) must extend the matching CSP allowlist in the same commit; otherwise the browser
will silently block the resource.

## Local setup

Use Node.js 24 LTS and pnpm 9.

```bash
docker compose up -d
bash deploy/local-test-databases.sh ensure
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

Individual engine requests can still fail transiently during a slot cutover. `packages/engine-client`
retries an idempotent `GET` exactly once (after a 300 ms pause) when it classified the failure as
retryable (network/timeout, HTTP ≥ 500, 429); mutations are never retried.

`GET /v1/ready` is the Caddy slot-health probe for this Nest process. HTTP 503 only when the
slot is draining (`SIGUSR1` / `isAccepting() == false`) or commerce PostgreSQL is down. HTTP 200
with `status: "ok"` and `database: "up"` means the process can accept HTTP. The JSON still
includes `engine: "up"|"down"` as telemetry; a down Control API origin (`127.0.0.1:8790`) must
not depool `3000`/`3001`. `GET /v1/health` stays HTTP 200 even when degraded (`ok: false` if
database or engine is down) and is what the public status page reads.

Routes that need the Control API (account, keys, live balance, admin engine views) still return
their own `503 "engine is temporarily unavailable"`. Blue-green drain still flips `/v1/ready` to
503 so Caddy depools the old slot.

When a retryable engine failure reaches `apps/api`, the account/payments controllers log the
original engine error at warn level (message, HTTP status, retryable flag, provisioning cause)
before answering the generic `503 "engine is temporarily unavailable"` — the public text is
deliberately uninformative, so incident diagnosis starts from that log line. The same holds for
the `502` (invalid engine response) and the uncaught-error `500`: an unlogged 5xx is
indistinguishable from a browser-side failure when a customer reports a dashboard section that
will not load. Typed HTTP exceptions (2FA required, policy conflict) are deliberate answers and
are not logged as failures.

A brand-new account provisions its idempotent engine mapping and stored scalar price during its
first dashboard load. Parallel section requests are serialized by the user-scoped PostgreSQL
advisory transaction lock, and the compare-and-set to `active` cannot overwrite a concurrent
administrative disable. There is no policy confirmation step: once the engine account exists with
the requested scalar, the mapping is usable. The dashboard additionally waits out a `503` on an optional
section (keys, ledger, usage) behind the skeleton for a few short attempts
(`apps/web/src/lib/provisioning-retry.ts`) before showing the "could not be loaded" notice, so a
first-time customer is not told their account is broken when it is one second old.

Run the real PostgreSQL checkout/payment and affiliate money tests with:

```bash
TEST_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/commerce \
TEST_SALES_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/sales \
  pnpm test:integration
```

`pnpm test:integration` fails closed if either DSN is missing. It migrates commerce
and sales, then runs `@claude-api/db` / `@claude-api/commercial-api` plus the
`sales-db` / `sales-api` SQL suites. `pnpm test` without those variables still
skips the SQL suites. The `sales` database lives on the same compose Postgres as
`commerce` (one instance, extra database), matching production and the watchdog.

Payment providers sit behind a provider-neutral adapter. Every adapter must verify
the provider event using its authoritative API and persist the webhook's globally unique event ID.
Only then may it create a payment and enqueue an engine credit. The worker uses the payment ID as a
stable, idempotent engine credit reference. A verified refund or chargeback changes the payment
state and records its engine compensation in the same PostgreSQL transaction. A never-claimed
positive credit is canceled; a credit that may already have reached the engine is first driven to
`confirmed`, then a separately leased worker sends exactly one negative adjustment under
`refund:<payment-id>`. This ordering handles a lost credit response without guessing whether the
top-up landed, and the refund remains durable while either engine call is retried. Provider
specifics are in `docs/commerce/PLATEGA_INTEGRATION.md`
(default), `docs/commerce/CRYPTOMUS_INTEGRATION.md` and `docs/commerce/DIGISELLER_INTEGRATION.md` (disabled).

Email/password authentication, authorization invariants and the future email/Google provider
boundaries are documented in `docs/commerce/AUTHENTICATION.md`.
Transactional email and self-hosted SMTP configuration are documented in `docs/commerce/EMAIL_INTEGRATION.md`.

B2C/B2B scalar defaults, optional per-provider overrides, OpenKeys/service boundaries and the
durable delivery queue are documented in `docs/commerce/PRICING.md` and
`docs/commerce/PRICING_MODEL.md`. Commerce persists desired values, while the engine account is the
request-time price authority. The worker delivers one independently leased `engine_pricing_jobs`
row per default or provider target; terminal updates fence on the exact claimant and requeue when
the desired value changed during the HTTP call.

The former policy/catalog/switch/release/funding lanes and their Stage 5–9 workers are deleted.
Their applied migrations remain immutable history, but their tables are neither a dormant feature
nor a rollback API. The exact retention, rollback-release, backup and drop gates are in
`docs/ops/PRICING_RETIREMENT.md`; historical stage documents are non-executable incident context.

Key issuance has no pricing-policy handshake. It first materializes the scalar engine account if
needed, then issues the key and persists only its non-secret identity and mask. If persistence
fails, commerce disables the just-issued engine key as compensation before returning an error.

The commercial admin API exposes the live managed surface:

```text
GET       /admin/business-users/{id}/pricing
PATCH     /admin/business-users/{id}/pricing
GET       /admin/users?limit=...&offset=...&sort=...&dir=...
POST      /admin/business-invites
GET       /admin/business-invites/{id}/link
POST      /admin/business-invites/{id}/revoke
POST      /admin/business-invites/{id}/resend
POST      /admin/users/{id}/provisioning-repair
POST      /admin/users/{id}/referral-partner
GET       /admin/referral/partners
POST      /admin/referral/partners
PATCH     /admin/referral/partners
GET       /admin/referral/requests
POST      /admin/referral/requests/{id}/decision
GET       /admin/referral/payouts
POST      /admin/referral/payouts/{id}/decision
GET       /admin/pipeline-health
GET       /admin/finance/paying-users?days=1|7|30&limit=...&offset=...&funding=payments|manual|bonus|all|spenders&include_usage=true|false
GET       /admin/finance/engine-spend?days=1|7|30
GET       /admin/events  (SSE invalidation feed)
```

`GET /admin/users` keeps search, filters, sorting and pagination in commerce PostgreSQL, then
adds live aggregate balance/spend from the engine. Each row also carries the additive
`provider_spend_30d` object: exact decimal nanoUSD strings for `anthropic_nano`, `openai_nano`,
`google_nano`, `kimi_nano` and `other_nano`, derived from the same immutable
`pricing_usage_events` half-open 30-day window as `spent_30d_usd`. Null, legacy and every provider
outside the four named rails stay in `other_nano`; the producer never relabels that residual as an
image-model total. The object is a read-only reporting projection and does not become a second
money authority.

`GET /admin/events` is an additive managed-admin SSE contract for cache invalidation, not a data
replica. One process-wide PostgreSQL listener consumes the commit-bound
`commerce_admin_changes` channel and maps its allowlisted table name to bounded resource prefixes
such as `/admin/users`, `/admin/dashboard` or `/admin/finance`. The process starts and awaits one
shared listener during module initialization; browser connections only subscribe to its in-memory
fanout, so concurrent first streams cannot open duplicate listeners and no initial refetch can race
ahead of `LISTEN`. Each stream receives an initial `resync`, real `change` events and transport-only heartbeats.
PostgreSQL notifications are intentionally non-durable, so reconnecting the listener publishes a
full commerce-owner `resync`; the consumer then refetches only matching mounted resources.

`POST /admin/users/{id}/provisioning-repair` is a bounded reconciliation for an existing mapping,
not a second provisioning implementation. It reads the mapped account from the engine before the
commerce mutation, requires the engine account to be `active`, and requires its default multiplier
to equal both commerce copies. A row lock then changes only that exact `pending|error` mapping to
`active`, clears its old error and writes one audit event; disabled users/accounts, missing or
changed identities, and pricing drift fail closed. A second engine read proves the external state
did not move during the repair. Repeating a successful request returns `already_active` and adds no
duplicate audit event. The endpoint never creates an account, imports an identity, or changes a
price.

`GET /admin/finance/engine-spend` is the fleet-wide counterpart of the paid-customer control room.
It reads the engine's operator projection `GET /spend-stats` once (windows `d1`/`d7`/`d30`) and
joins it with the commerce `engine_accounts → users` directory, so the admin page can show the
per-model and per-provider spend of the whole fleet and separate three account classes: `client`
(has a commerce user), `openkeys` (issued through the OpenKeys portal, recognized by handle) and
`internal` (service/manual engine accounts). The last two exist in no commerce table at all, which
is why their spend is invisible to every other finance endpoint. Amounts here are the engine's own
USD numbers (`charge_usd` — after the account multiplier, `real_usd` — provider list price), not
commerce nanoUSD strings: this endpoint is an operator projection, not a money authority.

`GET /admin/request-analytics/summary`, `GET /admin/request-analytics`, and
`GET /admin/request-analytics/logical/:id` are dedicated managed-admin request analytics producers.
They use `AdminGuard`, return `Cache-Control: no-store`, validate half-open 30-day windows/keyset bounds,
and call the engine only through `packages/engine-client`. The engine privacy projection contains no
account/key identity in v1, so commerce does not invent an owner join. These routes are separate from
Engine Spend and are never public/customer APIs.

`GET /admin/finance/paying-users` is the read-only producer for the paid-customer control room.
It includes a user with at least one `payments.status='paid'` row **or** at least one manual engine
top-up (`pricing_usage_topups.source='manual'`) whose ref is not `admin-credit:*`: these are explicit
external-money credits granted straight in the engine. Payment-sourced engine
top-ups are deliberately excluded from that sum: `payments` is their authority, so counting both
would double the same deposit. Bonus top-ups (welcome/promo/admin-credit) are never money. New
admin credits are stored as `source='bonus'`; historical immutable admin-credit rows with
`source='manual'` are excluded by ref rather than rewritten. Rows and summary expose
`manual_paid_nano`/`manual_topups_count` next to the payment counters.
`pricing_usage_topups` is an immutable reporting copy of engine top-ups; it is not a balance and
never drives one. Commission classification uses the stricter `isFreeCreditRef` whitelist.

`funding` selects the cohort by funding authority: omitted preserves the historical payment/manual
union; `payments` requires a lifetime confirmed provider payment; `manual` requires lifetime
qualifying manual top-up(s) and no provider payment. `bonus` instead requires no lifetime paid
payment, no qualifying lifetime manual top-up, and positive spend in the selected window where
every immutable event has a valid free-first split and the whole window is free-funded
(`real_funded_nano=0`, no invalid amount). A bonus top-up is neither required nor sufficient.
Current balance, model names and other heuristics never classify this cohort. `all` is the union of
the money cohort and strict bonus-only.
Additive `spenders` includes every commerce user with positive `pricing_usage_events.amount_nano`
in the selected window, regardless of lifetime payments/manual credits or mixed, other, legacy and
unattributed funding evidence; a top-up without spend remains excluded. As a cohort selector,
`funding` narrows both rows and summary.

Rows add `funding_kind` (`payments|payments_and_manual|manual|bonus_only|spend_only`) and exact
selected-window `paid_funded_spent_nano`, `bonus_funded_spent_nano`, `other_funded_spent_nano`, and
`unattributed_spent_nano`. A usage row keeps the full billed actual in `amount_nano`; the exact
engine-pool shortfall is `other_funded_spent_nano`, while paid and bonus funding split only the
collected remainder. Shortfall can therefore never make a row `bonus_only`. Zero-money spenders
retain `bonus_only` only under the strict proof above; all other zero-money positive spenders are
`spend_only`. Summary adds `bonus_only_users`,
`bonus_only_spent_nano`, and `cohort_users`. Backward-compatible `paying_users` continues to count
only money-funded users for every cohort; existing lifetime paid/manual and provider summary fields
keep their semantics.

Provider attribution is `pricing_usage_events.provider_id`, copied from exact engine ledger evidence
with no model inference. All money is a decimal nanoUSD string;
provider `other` still retains missing/unknown provider evidence, independently from the new funding
`unattributed_spent_nano`. Page, count, and cohort summary use one half-open
`[window_end-days, window_end)` cutoff inside one read-only `REPEATABLE READ` commerce snapshot.
Search, status and provider filters continue to narrow only the paged rows/count; the cohort-selected
summary remains fleet-wide.

Live model usage is explicitly opt-in through the closed literal query
`include_usage=true|false`; omitted and `false` preserve the DB-only response, perform no engine calls,
and expose neither `usage` nor internal engine account IDs. With `include_usage=true`, after DB
pagination (maximum 100 rows), commerce collects the distinct `engine_account_id` values from that
user's selected-window events. It uses the current `engine_accounts` mapping only as a fallback for a
money-funded row with no window event. Every collected account calls authoritative
`EngineClient.getUsage(accountId, "<days>d")` under one page-wide concurrency limit of four and a
fixed five-second page deadline. The shared abort signal cancels in-flight calls without retry; queued
calls are not started after expiry, so an engine outage degrades coverage instead of extending endpoint
latency or leaving an unbounded background fanout.

The opt-in response adds a deliberately bounded `usage` projection rather than the full engine
response: `window`, account coverage counts, aggregate request/official/charged totals, and models
grouped only by the exact `(provider, model)` pair. Provider is passed through from `EngineUsage`; an
absent optional value is serialized as `null`, without deriving it from the model ID or promising
anything about how the engine labels legacy rows. Engine request/token counters are accepted only as
safe JS integers, then summed with `BigInt` and serialized as decimal integer strings; exact nanoUSD
amounts remain decimal strings throughout. Account IDs, time bounds, buckets, daily series and key
details from `EngineUsage` are never copied into the HTTP response. Coverage is `complete` only when
every collected account succeeds, `partial` when only a subset succeeds, and `unavailable` when none
succeeds or no account is known. Partial totals contain only successful accounts and never masquerade
as complete. The engine's relative usage window is fetched after the commerce snapshot and is not
claimed to share its exact cutoff. `apps/admin` must consume this expand-only producer contract only
after the exact producer SHA has a green `deploy/watchdog` verdict.

The former Stage 5–9 capture/activation/orchestration, service-inventory, policy, catalog and
switch admin routes are removed. They must not be restored as rollback helpers; their schema is
retained only under `docs/ops/PRICING_RETIREMENT.md`.

The live pricing admin path is scalar: `PATCH /v1/admin/business-users/:id/pricing` atomically
persists the B2B default plus provider overrides and enqueues independently fenced
`engine_pricing_jobs`; read surfaces return those persisted desired values without substituting a
hardcoded B2C default. `GET /v1/admin/pipeline-health` exposes delivery/drift status. Official
provider tariffs use the engine's `/admin/pricing/tariffs*` surface; new model publication follows
`docs/ops/MODEL_RELEASE_CYCLE.md` without any release head or policy generation.
## Authenticated client API

All private routes use the HttpOnly session cookie and derive the owner from that session. Engine
account IDs and the Control API key are never accepted from, or exposed to, the browser.

```text
GET    /v1/account                 live engine balance, reserved amount, spend, markup and status
GET    /v1/account/ledger?limit=50 live engine top-ups and charges (limit 1..1000)
GET    /v1/api-keys                masked keys with live status and per-key spend
POST   /v1/api-keys                {"label"?: "production"}; raw sk-pool key returned once
DELETE /v1/api-keys/{id}           disable an owned key by commercial UUID
GET    /v1/referral                account-bound partner state and cabinet snapshot
POST   /v1/referral/team-invitations
DELETE /v1/referral/team-invitations/{inviteId}
PATCH  /v1/referral/team
POST   /v1/referral/requests/commission
POST   /v1/referral/requests/b2b
POST   /v1/referral/referrals/business-pricing
PATCH  /v1/referral/wallet
```

`GET /v1/account` also returns the authenticated customer's stored scalar pricing view. Provider
overrides are an operator surface and are not inferred from model names. Commercial operator routes
use a separate `COMMERCIAL_ADMIN_KEY`; they create email-bound or copy-only B2B invitations,
revoke/rotate them, and manage the B2B default plus provider overrides. That key is never a client
session or an engine Control API credential.

## Commerce partner-program boundary

Commerce is the only browser boundary for the account-bound partner program. `GET /v1/referral`
derives `commerceUserId` from the authenticated session and returns one of three explicit states:
`unavailable`, `disabled`, or `active`. The request body and query never select another partner.
An unavailable account receives no membership or financial data; the Dashboard presents the
program terms and the operator-contact action instead of creating a public application.

Team invitations, Team edits, B2B actions, requests and admin onboarding identify people by the
current email of an existing active Commerce account. Commerce normalizes and resolves that email
to `users.id`, then sends only the UUID to Sales under `SALES_CONTROL_KEY`. Sales remains the
membership and money authority and never stores an email snapshot. On reads, Commerce batch-loads
the current account projection and adds email, customer type and exact default/provider discount
terms where the view requires them. Before returning a browser response it removes Commerce UUIDs,
Sales partner IDs and parent/grant-source IDs. A temporarily unavailable account projection is
shown as missing email data; an internal ID is never substituted as the product identity.

The server-side `ReferralSalesClient` calls only `/v1/internal/referral/*`, validates every Sales
response with a strict local Zod schema and has a six-second timeout. Mutations are never retried by
this client. The caller supplies a scoped `Idempotency-Key` for request creation and direct B2B
pricing; the underlying Sales/Commerce operation remains the replay authority. Sales validation,
ownership and conflict statuses `400`, `403`, `404`, `409`, `422` and `429` are preserved. A Sales
authentication failure, transport error, timeout, 5xx or invalid response becomes a generic 503 and
does not misclassify the Commerce customer session.

The admin routes use `AdminGuard` and the same managed-admin Caddy boundary as the other
`/v1/admin/*` routes. Operators can onboard by email or from `POST
/v1/admin/users/:id/referral-partner`; both paths resolve the authoritative Commerce account before
Sales changes membership. Initial and editable terms are direct commission, Team ceiling, Team
invitation right, B2B self-service ceiling and B2B delegation right. A Team parent never selects a
member's direct platform commission; it edits only the retained edge share and delegated authority
within its own ceilings.

This consumer contract depends on the additive Sales request identity field from exact
production-GREEN producer SHA `a57b582b61a0d85cd42b90bdfce705611889fda9`. Nullable
`customerCommerceUserId` on request views lets Commerce restore the authoritative customer email
after a reload. The consumer keeps the field strict so an older or malformed producer fails
unavailable instead of silently showing a wrong identity.

Engine provisioning is recoverable: the stable handle `user:<commercial UUID>` makes account
creation idempotent. API-key revocation uses the engine's non-secret `key_id`; PostgreSQL stores the
commercial UUID, engine `key_id`, and mask, never the usable key. A key issued by the engine is
immediately disabled as compensation if its commercial record cannot be committed.

The target Google/GitHub B2C flow always records the anti-fraud profile and flags at OAuth
sign-in, then atomically records the exact `$5.000000000` amount with the anti-fraud claim and
sends one idempotent credit using `signup-bonus:<commercial UUID>`. The claim runs only against
an engine account that is freshly read as `active`: managed provisioning confirms asynchronously
via the worker, so the claim may be deferred past sign-in and is retried from both the next
OAuth sign-in and `AccountService.ensureEngineAccount` on account access. It can fund every
model allowed by the B2C policy. Password accounts are ineligible even when their
address is hosted by Gmail. Failed credit compensation clears both the claim and its amount.
Recovery never re-derives a nominal from OAuth or current pricing: it repeats the stored granted
amount under the same reference. A granted pre-0034 row with `bonus_amount_nano IS NULL` means the
historical `$4.000000000`; an ungranted or absent row means no credit. Administrative revocation
uses the same exact stored amount. B2B invitation provisioning has no grant to repeat.

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
- A terminal refund/chargeback atomically records `payments.status` and any required
  `engine_adjustments` row. Adjustment claims wait for the paired positive credit to confirm,
  terminal/retry writes fence on a unique worker lease, and the negative engine operation is
  idempotent by payment ID. A refunded payment may never depend on a best-effort in-memory debit.
- B2C usage rows are deduplicated by `(engine_account_id, ledger_entry_id)` and accepted only after
  exact ledger validation. Sales receives the scalar feed's exact referred-B2C
  `real_funded_nano`; welcome-funded usage and engine-pool settlement shortfall are excluded. Every
  new charge also persists the engine ledger's authoritative top-level
  `provider`; after the live cursor the worker repairs pre-column rows in bounded resumable pages
  from the retained 30-day ledger. Recovery algorithm v2 retries legacy `NULL`, provisional
  `unattributed`, and older terminal `unavailable` rows only when
  `provider_recovery_version < 2`. It promotes exact recovered evidence to
  Anthropic/OpenAI/Google and records an exhausted v2 attempt as `unavailable`; both outcomes store
  version `2`, and newly ingested exact providers start at that version. Model names never
  participate in recovery.
- Account-default and provider pricing changes are persisted with durable delivery jobs before the
  worker calls the engine. Each terminal write is fenced by the claim's `locked_by` and monotonic
  attempt; a stale worker cannot confirm or retry a lease that another worker now owns. If the
  desired value changed in flight, confirmation requeues the new value instead of acknowledging
  the obsolete delivery.
