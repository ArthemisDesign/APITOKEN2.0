# SALES_PORTAL.md — sales arm (partners.apitoken.sale)

The repository's third bounded context (after the engine and commerce): a **multi-level affiliate
program for salespeople**. A separate product, separate domain, separate DB, separate visual
style (not connected to apitoken.sale in any way). UI brand: **APIToken Partners**.

```
engine (Rust)  ←Control API─  commerce (apps/api + worker)  ←internal sales API (SALES_CONTROL_KEY)→  sales (sales-api + sales-web)
```

## What this is

- Sign-in and onboarding — **only via Telegram** (the official Login Widget; the bot is configured
  via `TELEGRAM_BOT_TOKEN`/`TELEGRAM_BOT_USERNAME`, the widget domain is bound in BotFather with
  `/setdomain`). A new partner is created ONLY through an invite issued to their Telegram username:
  the admin (Onboarding tab, root salespeople) or a partner (Team tab, sub-salespeople) enters the
  `@username` and sends the person a link `partners.apitoken.sale/register?invite=CODE`; the person
  confirms sign-in via Telegram — the account is immediately active, no password/email needed.
  The email/password fields in partners are legacy from the first wave.
- A partner receives a **referral code** and a link `https://apitoken.sale/register?ref=CODE`.
- Users who arrive via the link are attributed to the partner. The partner earns
  `commission_bps` on the **spend** (charge-ledger) of their users.
- **Multi-level:** a partner can invite sub-partners (invite link
  `partners.apitoken.sale/register?invite=CODE`). From a sub-partner's commission their parent
  receives `sub_commission_bps` — a "percentage of a percentage", a chain up to 10 levels deep.
- Terms (bps) are individual per partner, set in the admin panel.
- Payouts: the partner submits a request from their available balance; the admin
  approves/rejects/marks it paid.

## Components

| Path | What | Port (dev) |
|---|---|---|
| `packages/sales-db` | its own PostgreSQL DB `sales` (Drizzle, own migrations, advisory-lock migrate) | — |
| `apps/sales-api` | NestJS/Fastify backend: auth, partner dashboard, admin panel, sync loop, email outbox | 3100 |
| `apps/sales-web` | Next.js frontend: landing page, dashboard, `/admin` | 3200 |

## The sales ↔ commerce boundary (the only one)

The boundary is **two-sided**, both directions under a single server key `SALES_CONTROL_KEY`
(header `x-api-key`, timing-safe comparison). The key is the same one in the env of both services
(`/etc/apitoken/api.env` and `/etc/apitoken/sales.env`); without it the boundary is disabled, and
the two sides behave differently: the commerce feed without env responds **404** (the guard hides
the endpoints), sales-internal without env — **401**.

### Commerce → Sales: feeds and profiles (`apps/api/src/sales-feed.controller.ts`)

`@Controller("internal/sales")` behind the `SalesFeedGuard`. Cursor-based `after_id` model like the
engine ledger feed; rows younger than 10 s are hidden. Attribution and usage writers additionally
serialize `bigserial` allocation through a table lock, so commit order cannot expose a larger id
before a smaller in-flight id; the lag remains a rolling-deploy safeguard for the previous binary.
Garbage in `after_id`/`limit` is not an error but a default (cursor 0, default limit): parsing consumes
the whole decimal token, and `after_id` above PostgreSQL `bigint` is rejected to the default instead
of reaching SQL as an out-of-range value.

- `GET /v1/internal/sales/attributions?after_id&limit` (default limit 500, max 1000) — from
  `referral_attributions` (written at registration with `referralCode`, unique by user_id).
  Response `{items:[{id,userId,code,createdAt}]}`; there is no `nextCursor` — the reader's cursor
  is the page's max `id` (rows come in ascending `id` order).
- `GET /v1/internal/sales/usage-events?after_id&limit` (default 1000, max 2000) — from
  `pricing_usage_events`; the cursor is the `feed_seq bigserial` column (migration 0012). Response
  `{items:[{id,userId,amountNano,providerId,accountClass,pricingMode,paidFundedNano,
  commissionEligible,snapshotDigest,occurredAt}], nextCursor}`; money fields are decimal strings.
  The current producer returns only this scalar-compatible shape. For rollback and historical
  replay, the consumer also accepts the former optional
  `officialNano`/`chargedNano`/`bonusFundedNano`/`otherFundedNano`/`releaseGeneration`/
  `releaseDigest` fields. The consumer recognizes three forms:
  - **scalar (live since 2026-08-10)** — the only form the current producer emits. `providerId` is
    filled in, every attribution field (`accountClass`, `pricingMode`, `paidFundedNano`,
    `commissionEligible`, `snapshotDigest`) and the whole v2 lineage are `null`, and `amountNano`
    is already narrowed by the producer to `real_funded_nano` — the customer's own money — so it is
    the commission basis as-is. **`providerId` is not part of attribution**: it is informational and
    may be present or absent without making the row attributed. The consumer discriminates on
    `accountClass`, not on `providerId`. This is the shape the old all-null legacy rows also parse
    into, so the historical tail and the live stream share one path.

    A wire-exact example of this row lives in `tests/contracts/sales-usage-feed.golden.json`, and
    both ends assert against that same file — the producer that it serializes it, the consumer that
    it accepts it. Changing the shape means changing the golden and rolling both sides together.
    The rule exists because on 2026-08-10 both suites were green in isolation while the producer
    emitted `providerId` with null attribution and the consumer rejected exactly that combination
    (`usage attribution must be entirely null or complete`); the partner sync stopped for five
    hours at cursor `usage_events=105302` and no commission accrued in that window.
  - **schema v1 `policy_v1`** (retired producer, historical rows): `accountClass=b2c`, `pricingMode=track`,
    `commissionEligible=true` and a positive `paidFundedNano`; `amountNano` exactly equals this
    paid portion. Static, B2B/service, unsupported schema, zero-paid, and spend of a non-attributed
    user do not create an item.
  - **schema v2 `release_v2`** (retired producer, historical rows): eligibility independent of
    pricing mode — referred B2C authority,
    `commissionEligible=true` (computed by the commerce writer from `account_class='b2c'` + exact
    `paid_funded_nano>0`) and the full release lineage (`officialNano`, `chargedNano`,
    `bonusFundedNano`, `otherFundedNano`, `releaseGeneration`, `releaseDigest` non-null);
    `amountNano` equals the exact `paidFundedNano`, `pricingMode` is always `null` (no synthetic
    `'track'`). Bonus-only (`paidFundedNano=0`), B2B, OpenKeys and service v2 rows were not emitted.
  Historical all-null wire rows use the legacy `real_funded` free-first projection. A usage row is
  joined to referral attribution only when `occurred_at >= attributed created_at`, so assigning a
  referral later cannot retroactively earn commission on earlier spend. The current
  producer applies the limit to source rows before filtering,
  so `nextCursor` advances over the watermark of the whole page, including the
  static/service/unreferred tail, and it is never rescanned indefinitely.

  The producer now emits only the scalar form. The consumer deliberately retains the historical v1
  and v2 parsers/writers for expand-only replay: every row form still routes to exactly one writer
  (scalar/v1 → `recordReferredSpend`, v2 → `recordReferredSpendV2`), and readers aggregate both
  commission tables. This compatibility does not make either retired form a live producer contract.
- `GET /v1/internal/sales/topups?after_id&limit` (default 500, max 1000) — paid
  `payments`; the cursor is epoch microseconds from `paid_at` (not `feed_seq`: payment happens
  after the insert, and a stale `feed_seq` would fall out of the cursor forever). Also filtered by
  attribution and by `paid_at >= attributed created_at`. Response
  `{items:[{id,paymentId,userId,amountNano,paidAt}], nextCursor}`. The producer reads one look-ahead
  row and fails the page without cursor advance when equal `paid_at` timestamps cross the page
  boundary: a timestamp-only cursor cannot resume inside that group without losing rows. A future
  additive feed revision will replace this guard with a monotonic paid-transition cursor.
- `POST /v1/internal/sales/referral-discount` — records the salesperson's discount "floor" for a
  referral as partner attribution. Body `{userId, floorBps (0..9500), override?, actorId?}` →
  `{applied, multiplierBp}`. The floor does not move any price: B2C pricing is the stored account
  scalar plus optional provider overrides, while this field records the rate promised by the
  partner. `multiplierBp` is always `null` and no engine pricing job is enqueued. The floor stays
  **monotonic** (`GREATEST`, the best for the customer) across its three writers (promo, partner
  link, sales feed); `override=true` is an absolute write that can lower it (a partner or admin
  from the sales dashboard), `floorBps=0` is an explicit reset. Only b2c profiles
  (`applied:false` otherwise). Idempotent. The route remains live because partner and admin views
  display the promised rate even though it is not a pricing authority.
- `POST /v1/internal/sales/referral-profiles` — referral profiles for the partner's storefront.
  Body `{userIds: uuid[] (max 500)}` → `{items:[{userId, customerType (b2c/b2b), multiplierBp,
  discountPercent, referralFloorBps, cumulativeTopupNano, balanceNano, status}]}`. Only for an
  explicit list of user_id values that sales-api builds from the referrals assigned to the partner —
  a partner sees only their own. `balanceNano` and the live `multiplierBp` are read from the engine
  (the money authority) with parallelism 8; an unavailable engine account does not take the page
  down — the fields degrade to `null`/values from `customer_profiles`.
  `referralFloorBps` is display/partner-attribution data and `cumulativeTopupNano` is reporting;
  neither is used to calculate the customer's price.

Consumers on the sales side (`apps/sales-api`, reach commerce via `COMMERCE_BASE_URL`):

- `sync.service.ts` — sync loop over cursors (stored in the sales DB, interval `SYNC_INTERVAL_MS`,
  default 60 s): attributions → assignment of the user to a partner + atomic claim of the one-time
  discount link (the winner receives the floor via `POST referral-discount` — either for the first
  time or as an idempotent backfill if the synchronous application at registration failed);
  topups → `referred_topups` (history/analytics only, create no commissions); usage events →
  commissions (idempotent by `commerce_event_id`). The live scalar row and historical policy-v1
  row use `recordReferredSpend`; a historical release-v2 row
  (`pricingMode=null` + full lineage) goes to the schema-v2 writer (`recordReferredSpendV2`,
  `packages/sales-db/src/commissions-v2.ts`). Feed ids, cursors and money strings must be canonical
  non-negative decimals within PostgreSQL `bigint`; rolling-deploy numeric-id compatibility accepts
  only JavaScript safe integers. Unsafe numbers, leading-zero strings and bigint overflow reject the
  entire page before cursor advance. One event is processed by exactly one writer, and an
  incomplete or mixed form halts the page before cursor advance (fail closed, no silent fallback
  to v1). An attributed v1 payload is accepted only in the current full B2C schema-v1 form,
  and its exact paid basis and immutable fields are atomically preserved in
  `partner_usage_events`. Events that arrive before their user's attribution are buffered: v1 — in
  `pending_referral_events`, v2 — in `pending_referral_usage_events_v2` (deterministic
  `commerce_ref` of the form `usage-v2:<commerce_event_id>`), and are caught up by replay without
  loss of fields (`reconcilePendingReferralEvents` + `reconcilePendingReferralUsageEventsV2` on
  every tick). Both writers take the same transaction advisory lock derived bijectively from
  `commerce_event_id` and inspect all four recorded/pending v1/v2 stores before writing. The common
  immutable identity is user + exact paid basis + event timestamp: an exact cross-schema replay is
  duplicate/buffered, any divergence or multiple owner rows fails closed, and one commerce event
  cannot create commission in both schemas. The storage writer repeats the producer's temporal
  gate: `occurred_at`/`paid_at` before `referred_users.attributed_at` is skipped, including deletion
  of an older buffered row after attribution becomes visible. An exact same-schema repeat is
  idempotent, an all-null row
  can be enriched with attribution once during rolling retry, and a conflicting immutable replay
  is rejected. Deposit replay additionally requires exact payment id, user, partner, amount and
  timestamp equality. A 404 from the feed (the commerce side
  is not deployed yet) is not an error — retry on the next tick; the cursor advances
  only over successfully processed rows (at-least-once).
- `commerce.service.ts` — `referralProfiles` for the partner's storefront (**best-effort**: if
  commerce is unavailable an empty map is returned and the storefront degrades to local fields —
  spend/commission) and `setReferralDiscount` (sends `override=true`; **not** best-effort —
  transport errors are propagated to the caller; the partner must know the result).

Sales migration `0014_usage_attribution_buffer.sql` was delivered migration-first and expanded
`pending_referral_events` to include the writer. Legacy spend and deposit keep the fully
`NULL` form of the new fields. Attributed buffered spend must carry non-empty
`provider_id`/`snapshot_digest`, `account_class=b2c`, `pricing_mode=track`,
`commission_eligible=true` and a positive `paid_funded_nano` exactly equal to `amount_nano`.
The accompanying constraint on `partner_usage_events` forbids an attributed commission outside the
same B2C track authority; the application writer and replay now obey this form.

Migration `packages/sales-db/migrations/0015_paid_funded_commission_v2.sql` created separate
`partner_usage_events_v2`, `pending_referral_usage_events_v2` and `commission_entries_v2`. They have
no pricing-mode field: eligibility is set by the referred-B2C authority, `commission_eligible=true`
and a positive exact `paid_funded_nano`; the trigger ties the direct partner, the active parent
chain, the fixed bps values and integer-floor amounts. Usage/commission evidence is immutable. Old
rows and constraints are not rewritten. The historical v2 writer remains reachable only when a
stored/replayed v2 wire row arrives; the live producer emits scalar rows to the v1 store. All sum
readers (partner storefront, periods/payouts, analytics, admin panel) aggregate both stores via
`UNION ALL`, and the event forms do not overlap. These sales money tables are outside the pricing
retirement manifest; removing either store requires its own evidence and consumer migration.

### Sales → Commerce: promo and registration (`apps/sales-api/src/internal.controller.ts`)

Commerce calls sales-api at `SALES_API_URL` with the same `SALES_CONTROL_KEY`.

- `POST /v1/internal/promo/redeem` — redeeming a partner promo code (called from
  `apps/api/src/promo.service.ts`, public `POST /v1/promo/redeem`). Body
  `{code, commerceUserId}` → `{valueNano, partnerId, referralCode, redemptionRef, discountBps,
  alreadyRedeemed}`. Atomic and idempotent by (code, user): a repeat redemption by the same user
  returns the same `redemptionRef`, so the engine credit on the commerce side is idempotent by ref
  (retries are safe). One-time code; one promo per user (409); the code is unavailable if the
  partner is not active or the promo is disabled. Commerce continues on its own: credits the engine
  (up to 3 attempts), best-effort attributes an unassigned user to the code's owner, and with
  `discountBps>0` applies the discount "floor" with local retries — the async feed does **not**
  re-apply the promo discount (it derives the floor only from `partner_discount_links`).
  `discountBps` records the promised partner rate but does not change the B2C scalar price.
- `POST /v1/internal/partners/referral-discount` — atomic claim of a personal discount link. Body
  `{code, commerceUserId}` → `{discountBps}`. First-wins, idempotent by (code, user): the link is
  bound to the first owner in a single UPDATE and NEVER gives the discount to a second person; an
  ordinary ref code or a link redeemed by someone else → 0. Called from
  `apps/api/src/auth.service.ts` at the first activation of the engine account (password
  registration, email confirmation, OAuth) — synchronously, so the referral sees their rate from
  the first visit; fully best-effort
  (4 s timeout, failure → the async feed applies the owner's floor on the next tick).
  The link keeps attribution/commission and the promised-rate display, but it does not create a
  personal price.
- `GET /v1/internal/partners/resolve?code` → `{found:false}` or `{found:true, partnerId,
  referralDiscountBps}` — resolving the ref code of an active partner (`Cache-Control: no-store`).
  The endpoint is live, but the current commerce code does **not** call it: the claim endpoint
  above replaced the resolve+consume pair, closing the window where a read-only resolve handed the
  floor to several registrations of one link.

Rules: sales does not open the commerce/engine PostgreSQL and does not import `@claude-api/db`;
commerce symmetrically does not open the sales DB — everything goes through HTTP under the key.
Money amounts — only integer nanoUSD decimal strings; end-user emails are never given to a partner
(the dashboard shows only a masked user-id).

## Attribution on the main site

`apps/web`: a valid `?ref=CODE` on any main-site page is saved to first-party localStorage for
30 days and sent at registration as `referralCode`. Initial capture runs before visible navigation;
client-side route capture repeats it, and locale changes preserve the full query and fragment. The
latest distinct referral click wins, while revisiting the same code does not extend its expiry.
Commerce writes the code best-effort to `referral_attributions` (unique by user_id). Ref is also
passed through OAuth registration: the social buttons pass it in `oauthUrl` (`apps/web/src/lib/api.ts`),
`beginOAuth` saves the code in the OAuth transaction (it survives the redirect to the provider), and
`completeOAuth` for a **new** account writes the attribution. The current code also calls
`POST /v1/internal/partners/referral-discount` to claim a one-time link and record its promised
rate immediately. Ref affects attribution, commission and partner-facing display; the B2C price
remains the stored account scalar plus any provider override.

The complete product guide for the whole program (sign-in, attribution, commission, levels, wallet,
periods, dashboard, admin panel, languages) — `docs/sales/PARTNER_PROGRAM.md`.

## Payouts by periods

Half-month periods (1–15, 16–end, UTC), 7-day lock, 3-day payout window, auto-rollover of the
uncovered, and one integer threshold `SALES_MIN_PAYOUT_USD` (default/current `0`, inclusive when
nonzero), with payouts to the bound BSC wallet.
Computed from both commission tables (`commission_entries` + `commission_entries_v2`, UNION ALL —
the events do not overlap) + `payouts`, with no separate table. Full description —
`docs/sales/SALES_PAYOUT_PERIODS.md`. Code: `periods.ts` (+tests), `payout-periods.ts`, and the live
on-chain state machine in `apps/sales-api/src/payout` + `packages/sales-db/src/payout-batch.ts`.
Preparation revalidates amount/address under partner locks and pins the hot wallet; send/retry/poller
share a cross-process lock, persist exact hash/raw/nonce before broadcast and mark paid only from a
confirmed BSC receipt. `SALES_MIN_PAYOUT_USD` is also the execution threshold; the retired fractional
`PAYOUT_MIN_USD` knob does not exist.
The Sales Web send controls are an additional fail-closed safety layer: API money must be canonical
nanoUSD, row/recipient/batch/required totals must agree, both balances must be explicitly sufficient,
the current hot wallet must match the batch-pinned wallet, and no row may remain in `broadcast`.
Malformed money is rendered unavailable rather than as a false `$0.00`. The backend remains the
authority and repeats all irreversible checks under the cross-process send lock.

## Commission math (sales-db)

For the live scalar usage event, `A` is `amountNano`, already narrowed by commerce to the
customer-funded `real_funded_nano` after free-first accounting and settlement shortfall. Historical
v1 rows use the same writer; historical v2 rows use their exact `paid_funded_nano`. For a user of
partner P0:
- level 0: `A * P0.commission_bps / 10000` (integer floor);
- level N: `amount(level N-1) * Pn.sub_commission_bps / 10000` up the parent chain;
- stop: no parent, amount 0, after 10 entries (levels 0..9), or a suspended parent.
Entries are idempotent via the unique `commerce_event_id`; the calculation happens in the same
transaction as the event insert. Scalar and historical v1 events are written to
`partner_usage_events`/`commission_entries`; historical v2 rows go to
`partner_usage_events_v2`/`commission_entries_v2` (the trigger `enforce_commission_entry_v2_source`
fails closed on a row outside that chain). Before computing either schema, every traversed partner
row is held `FOR SHARE`, so parent/status/bps cannot change under the calculation; a repeated
partner id is an explicit cycle error and rolls back the whole event. Readers sum both stores via
UNION ALL.
Payout balance = confirmed commissions − (paid + active requests).

## Env (apps/sales-api)

`SALES_DATABASE_URL`, `SALES_TOKEN_ENCRYPTION_KEY`, `SALES_ADMIN_KEY`, `SALES_CONTROL_KEY`
(the same one as apps/api), `COMMERCE_BASE_URL` (production: the stable Caddy balancer
`http://127.0.0.1:8791`), `PUBLIC_SALES_BASE_URL`, `PUBLIC_MAIN_SITE_URL`, SMTP as in the worker (Brevo),
`SALES_SESSION_TTL_SECONDS`, `SALES_SESSION_CACHE_TTL_SECONDS`, `SYNC_INTERVAL_MS`. Full list —
`apps/sales-api/.env.example`.

## Sessions and their cache

Every request under `SessionAuthGuard` resolves the session via `AuthService.authenticate`. So that
resolution does not hit PostgreSQL on every request, successful resolutions are cached in-process for
`SALES_SESSION_CACHE_TTL_SECONDS` (default 30, 0 — disables the cache). Freshness contract:
logout, profile edits by the partner themselves, and admin patch/delete invalidate the cache
immediately (all of these are the same sales-api process); the only staleness window is a
status/profile change by a concurrent generation during blue-green overlap, bounded by the TTL.
Separately from the cache, the `partner_sessions.last_seen_at` write is throttled to one per 60
seconds per session by a predicate in the UPDATE itself (no extra round trip): the field is needed
for the "active partner" views, and sub-minute precision buys nothing there.

## Deployment (IN PRODUCTION since 2026-07-19)

https://partners.apitoken.sale is live. How it is set up on 84.32.48.2:

- DB `sales` (role `sales`) in the commerce Postgres (`deploy-commerce-postgres-1`, :5433).
  Migrations: `node <release realpath>/packages/sales-db/dist/migrate.js` with env from
  `/etc/apitoken/sales.env`. **Gotcha:** run via the dereferenced SHA path, not through the
  `current` symlink — the `isDirectExecution` guard compares realpaths and exits silently.
- systemd: `apitoken-sales-api.service` (:3100) and `apitoken-sales-web.service` (:3200,
  `next start -H 127.0.0.1` — bind ONLY to loopback). **Gotcha:** sales-web needs `AF_NETLINK`
  in `RestrictAddressFamilies`, otherwise Next crashes on `uv_interface_addresses`.
- **sales in the watchdog pipeline (auto-deploy).** Path class `wd_path_is_sales`
  (`apps/sales-api/*`, `apps/sales-web/*`, `packages/sales-db/*`, shared build files) with a separate
  baseline `/var/lib/apitoken/watchdog/sales.sha`. After green tests the watchdog calls
  `deploy/sales-deploy.sh <sha>`: promotes the tested candidate to an immutable release
  `/opt/apitoken/sales-releases/<sha>` → verifies the byte-for-byte append-only Sales migration
  history against `/var/lib/apitoken/watchdog/sales-database-migrations.manifest` → if and only if
  history grew, creates a fresh validated exact-SHA backup and runs the advisory-locked migrator →
  commits the new manifest only after migration success →
  atomic swap of `sales-releases/current` → restart of both units → health gate
  (`/v1/health` + `/` each 200, up to 60 s) → **symlink rollback** on failure. Status context —
  `deploy/sales`. sales has ITS OWN release root, NOT on the shared commerce `current` (that is
  commerce blue-green — do not touch). The units look at `sales-releases/current`.
  The first guarded rollout bootstraps its manifest only from the already-live immutable Sales SHA,
  never from an un-applied candidate. An app-only release skips the schema command entirely.
  Manual emergency deployment (if ever needed) — the same `sales-deploy.sh <sha>` from a candidate.
- Env: `/etc/apitoken/sales.env` (all keys: SALES_DATABASE_URL, SALES_TOKEN_ENCRYPTION_KEY,
  `SALES_ADMIN_KEY` — the key for signing in to /admin, SALES_CONTROL_KEY, SMTP Brevo). The same
  `SALES_CONTROL_KEY` is added to `/etc/apitoken/api.env` — this enables the feed.
- Telegram sign-in is enabled on the server: in `/etc/apitoken/sales.env` add
  `TELEGRAM_BOT_TOKEN` (from BotFather) and `TELEGRAM_BOT_USERNAME` (without @), run `/setdomain`
  → `partners.apitoken.sale` for the bot, then `systemctl restart apitoken-sales-api`.
  Until configured — `/v1/auth/telegram*` responds 503 and the site shows "sign-in unavailable".
- Caddy: vhost `partners.apitoken.sale` (`/v1/*`→:3100, everything else→:3200, same-origin cookies;
  the old `sales.apitoken.sale` — 301 to partners) and
  loopback `http://127.0.0.1:8791` — a stable health-gated origin for the commerce backend on top of
  the blue-green slots 3000/3001 (the analogue of 8790 for the engine); `COMMERCE_BASE_URL=http://127.0.0.1:8791`.
  `sales-deploy.sh` atomically brings the root-only production env to this address before restarting
  the API.
- Sync verified on live data: cursors walked the whole history of usage events and topups; the feed
  responds 401 without the key; the verify email actually went out via Brevo.

## Development ideas (not implemented)

- Server-side attribution cookie instead of localStorage.
- Promo materials in the dashboard (banners, UTM builder), click statistics storefront
  (today we only count registrations and spend).
- Notifications to the partner (email/TG) about new referrals and accruals.
- Personal landing pages/discount promo codes funded from the partner's commission.
