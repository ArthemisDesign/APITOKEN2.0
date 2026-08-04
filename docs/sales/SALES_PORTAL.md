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
engine ledger feed; rows younger than 10 s are hidden (the lag closes the bigserial/commit race).
Garbage in `after_id`/`limit` is not an error but a default (cursor 0, default limit).

- `GET /v1/internal/sales/attributions?after_id&limit` (default limit 500, max 1000) — from
  `referral_attributions` (written at registration with `referralCode`, unique by user_id).
  Response `{items:[{id,userId,code,createdAt}]}`; there is no `nextCursor` — the reader's cursor
  is the page's max `id` (rows come in ascending `id` order).
- `GET /v1/internal/sales/usage-events?after_id&limit` (default 1000, max 2000) — from
  `pricing_usage_events`; the cursor is the `feed_seq bigserial` column (migration 0012). Response
  `{items:[{id,userId,amountNano,providerId,accountClass,pricingMode,paidFundedNano,
  commissionEligible,snapshotDigest,officialNano,chargedNano,bonusFundedNano,otherFundedNano,
  releaseGeneration,releaseDigest,occurredAt}], nextCursor}`; money fields and `releaseGeneration`
  are decimal strings. The `officialNano…releaseDigest` fields are an expand-only schema v2
  addition: on legacy/all-null and policy_v1 rows they are `null`, and are removed only as the
  final contract step after the consumer has fully migrated. For immutable attribution the feed
  allows two forms:
  - **schema v1 `policy_v1`** (deployed consumer): `accountClass=b2c`, `pricingMode=track`,
    `commissionEligible=true` and a positive `paidFundedNano`; `amountNano` exactly equals this
    paid portion. Static, B2B/service, unsupported schema, zero-paid, and spend of a non-attributed
    user do not create an item.
  - **schema v2 `release_v2`**: eligibility independent of pricing mode — referred B2C authority,
    `commissionEligible=true` (computed by the commerce writer from `account_class='b2c'` + exact
    `paid_funded_nano>0`) and the full release lineage (`officialNano`, `chargedNano`,
    `bonusFundedNano`, `otherFundedNano`, `releaseGeneration`, `releaseDigest` non-null);
    `amountNano` equals the exact `paidFundedNano`, `pricingMode` is always `null` (no synthetic
    `'track'`). Bonus-only (`paidFundedNano=0`), B2B, OpenKeys and service v2 rows are not emitted.
  Historical rows without attribution temporarily remain all-null and use the
  legacy `real_funded` free-first projection. The limit is applied to source rows before filtering,
  so `nextCursor` advances over the watermark of the whole page, including the
  static/service/unreferred tail, and it is never rescanned indefinitely.

  Producer-first migration: the producer emits both schemas; the sales consumer is a dual-consumer
  (delivered): every event is routed by row form to exactly one writer
  (v1 → `recordReferredSpend`, v2 → `recordReferredSpendV2`), and readers aggregate both
  commission tables. The `track` field is not a target authority; removing the legacy v1 form is
  the final contract step after Stage 9.
- `GET /v1/internal/sales/topups?after_id&limit` (default 500, max 1000) — paid
  `payments`; the cursor is epoch microseconds from `paid_at` (not `feed_seq`: payment happens
  after the insert, and a stale `feed_seq` would fall out of the cursor forever). Also filtered by
  attribution. Response `{items:[{id,paymentId,userId,amountNano,paidAt}], nextCursor}`.
- `POST /v1/internal/sales/referral-discount` — the current legacy discount "floor" of a
  salesperson for a referral. Body `{userId, floorBps (0..9500), override?, actorId?}` →
  `{applied, multiplierBp}`. The customer stays b2c and follows the normal tier rules; the floor
  merely guarantees a price no worse than the salesperson's discount: effective mult =
  `min(tier-mult, 10000 − floorBps)`. By default the floor is **monotonic** (`GREATEST`, the best
  for the customer) — it is written by three independent sources (promo, partner link, sales feed);
  `override=true` is an absolute write that can lower it (a partner or admin from the sales
  dashboard), `floorBps=0` is an explicit reset. Only b2c profiles (business-b2b or no tier →
  `applied:false`). Idempotent; the multiplier is delivered to the engine via durable
  `engine_pricing_jobs`.
  This tier-linked contract is removed together with progressive pricing: target B2C gets the
  global 50%/provider/model policy, and the partner relationship affects commission but does not
  create a personal price. The route stays only for the producer-first compatibility period and is
  then removed as the final contract step.
- `POST /v1/internal/sales/referral-profiles` — referral profiles for the partner's storefront.
  Body `{userIds: uuid[] (max 500)}` → `{items:[{userId, customerType (b2c/b2b), multiplierBp,
  discountPercent, referralFloorBps, cumulativeTopupNano, balanceNano, status}]}`. Only for an
  explicit list of user_id values that sales-api builds from the referrals assigned to the partner —
  a partner sees only their own. `balanceNano` and the live `multiplierBp` are read from the engine
  (the money authority) with parallelism 8; an unavailable engine account does not take the page
  down — the fields degrade to `null`/values from `customer_profiles`.
  The target response removes tier-only `referralFloorBps`/`cumulativeTopupNano` from business
  logic and shows the effective B2C discount/source or the independent B2B policy. The current
  schema's fields may remain nullable during the expand-only migration, but the consumer does not
  use them for pricing.

Consumers on the sales side (`apps/sales-api`, reach commerce via `COMMERCE_BASE_URL`):

- `sync.service.ts` — sync loop over cursors (stored in the sales DB, interval `SYNC_INTERVAL_MS`,
  default 60 s): attributions → assignment of the user to a partner + atomic claim of the one-time
  discount link (the winner receives the floor via `POST referral-discount` — either for the first
  time or as an idempotent backfill if the synchronous application at registration failed);
  topups → `referred_topups` (history/analytics only, create no commissions); usage events →
  commissions (idempotent by `commerce_event_id`). The consumer is dual: a release-v2 row
  (`pricingMode=null` + full lineage) goes to the schema-v2 writer (`recordReferredSpendV2`,
  `packages/sales-db/src/commissions-v2.ts`), a policy_v1 track row and legacy all-null rows go to
  the v1 writer (`recordReferredSpend`); one event is processed by exactly one writer, and an
  incomplete or mixed form halts the page before cursor advance (fail closed, no silent fallback
  to v1). An attributed v1 payload is accepted only in the current full B2C schema-v1 form,
  and its exact paid basis and immutable fields are atomically preserved in
  `partner_usage_events`. Events that arrive before their user's attribution are buffered: v1 — in
  `pending_referral_events`, v2 — in `pending_referral_usage_events_v2` (deterministic
  `commerce_ref` of the form `usage-v2:<commerce_event_id>`), and are caught up by replay without
  loss of fields (`reconcilePendingReferralEvents` + `reconcilePendingReferralUsageEventsV2` on
  every tick). An exact repeat is idempotent, an all-null row
  can be enriched with attribution once during rolling retry, and a conflicting immutable replay
  is rejected. A 404 from the feed (the commerce side
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

Target migration `packages/sales-db/migrations/0015_paid_funded_commission_v2.sql` created separate
`partner_usage_events_v2`, `pending_referral_usage_events_v2` and `commission_entries_v2`. They have
no pricing-mode field: eligibility is set by the referred-B2C authority, `commission_eligible=true`
and a positive exact `paid_funded_nano`; the trigger ties the direct partner, the active parent
chain, the fixed bps values and integer-floor amounts. Usage/commission evidence is immutable. Old
rows and constraints are not rewritten. The dual-consumer checkpoint is delivered: the v2 tables are
no longer dormant — the schema-v2 writer (`commissions-v2.ts`) writes them in parallel with the v1
writer (routing by event form), and all sum readers (partner storefront, periods/payouts, analytics,
admin panel) aggregate BOTH schemas via UNION ALL — v1 and v2 events do not overlap, so there is no
double counting. Removing the legacy v1 tables and the v1 feed form is the final contract step after
Stage 9.

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
  In the target contract promo keeps credit/referral attribution but does not change the B2C price:
  `discountBps` becomes a deprecated producer field and is then removed producer-last after the
  consumers switch over.
- `POST /v1/internal/partners/referral-discount` — atomic claim of a personal discount link. Body
  `{code, commerceUserId}` → `{discountBps}`. First-wins, idempotent by (code, user): the link is
  bound to the first owner in a single UPDATE and NEVER gives the discount to a second person; an
  ordinary ref code or a link redeemed by someone else → 0. Called from
  `apps/api/src/auth.service.ts` at the first activation of the engine account (password
  registration, email confirmation, OAuth) — synchronously, so the referral sees their rate from
  the first visit; fully best-effort
  (4 s timeout, failure → the async feed applies the owner's floor on the next tick).
  The route and the discount links are legacy tier-linked surface; target registration does not
  call them. The referral link keeps attribution/commission but not a personal discount.
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
`completeOAuth` for a **new** account writes the attribution. The current code also calls the legacy `POST
/v1/internal/partners/referral-discount`; by Stage 9 this call is removed. In the target contract
ref only affects commission, and the B2C price is determined by global/provider/model policy.

The complete product guide for the whole program (sign-in, attribution, commission, levels, wallet,
periods, dashboard, admin panel, languages) — `docs/sales/PARTNER_PROGRAM.md`.

## Payouts by periods

Half-month periods (1–15, 16–end, UTC), 7-day lock, 3-day payout window, auto-rollover of the
uncovered, minimum `SALES_MIN_PAYOUT_USD` ($10), payouts to the bound BSC wallet.
Computed from both commission tables (`commission_entries` + `commission_entries_v2`, UNION ALL —
the events do not overlap) + `payouts`, with no separate table. Full description —
`docs/sales/SALES_PAYOUT_PERIODS.md`. Code: `periods.ts` (+tests) and `payout-periods.ts`. Sending
payouts (on-chain) is a separate upcoming system.

## Commission math (sales-db)

For the target schema-v2 usage event, `A` is the exact `paid_funded_nano` from the immutable
referred-B2C attribution; bonus-funded/B2B/OpenKeys/service/ineligible rows never reach the
calculation. Only for the historical all-null form
does `A` temporarily remain the legacy `real_funded` free-first projection. For a user of partner P0:
- level 0: `A * P0.commission_bps / 10000` (integer floor);
- level N: `amount(level N-1) * Pn.sub_commission_bps / 10000` up the parent chain;
- stop: no parent, amount 0, level > 10, or a suspended parent.
Entries are idempotent via the unique `commerce_event_id`; the calculation happens in the same
transaction as the event insert. Dual-consumer: v1 events are written to
`partner_usage_events`/`commission_entries`, v2 — to
`partner_usage_events_v2`/`commission_entries_v2` (the trigger `enforce_commission_entry_v2_source`
fail-closed rejects a row outside the active chain); readers sum both schemas via UNION ALL.
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
  `/opt/apitoken/sales-releases/<sha>` → sales-db migrations (advisory-lock, expand-only) →
  atomic swap of `sales-releases/current` → restart of both units → health gate
  (`/v1/health` + `/` each 200, up to 60 s) → **symlink rollback** on failure. Status context —
  `deploy/sales`. sales has ITS OWN release root, NOT on the shared commerce `current` (that is
  commerce blue-green — do not touch). The units look at `sales-releases/current`.
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
- Auto-payouts in USDT via a provider; minimum payout threshold.
- Notifications to the partner (email/TG) about new referrals and accruals.
- Personal landing pages/discount promo codes funded from the partner's commission.
