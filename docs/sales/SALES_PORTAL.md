# SALES_PORTAL.md — Sales financial authority for the partner program

> **Retired for browsers.** The partner cabinet lives in the customer Dashboard
> (`apitoken.sale/dashboard?view=referral`), and `partners.apitoken.sale` redirects every browser
> route there. This document now describes the Sales API and its data model, which remain the
> authority for partners, commissions and payouts, plus the still-served
> `admin.partners.apitoken.sale` operator surface.

The repository's third bounded context (after the engine and commerce): a **multi-level affiliate
program for salespeople**. It remains a separate database and server-side financial authority, but
Commerce accounts are the new membership identity and the canonical UI moves into the main
Dashboard/Admin. The legacy Sales domains stay online only through verified consumer cutover.

```
engine (Rust)  ←Control API─  commerce (apps/api + worker)  ←internal sales API (SALES_CONTROL_KEY)→  sales (sales-api + sales-web)
```

## What this is

- Membership identity is immutable `partners.commerce_user_id`. Commerce derives it from the
  authenticated session; neither browser nor email is accepted by Sales as proof of identity.
  An operator onboards an existing Commerce account immediately, or a partner invites an existing
  account by email after Commerce resolves it to UUID. Sales stores that UUID, not the email.
  Legacy Telegram login/session/application code remains reachable only during producer-first
  rollout and cannot enable a migration-0026 membership by itself.
- A partner receives a **referral code** and a link `https://apitoken.sale/?ref=CODE`.
- Users who arrive via the link are attributed to the partner. The partner earns
  `commission_bps` on the **spend** (charge-ledger) of their users.
- **Multi-level:** a partner can invite direct Team members by Commerce account. The exact
  `parent_override_bps` is the share withheld from that member's fixed gross direct commission;
  it is not an additive bonus. The remaining parent cut can itself be divided with the next
  ancestor. Across at most ten entries the net sum remains exactly the original direct gross pool.
  An older NULL edge retains the parent's `sub_commission_bps` only as a history fallback.
- **Team API:** Commerce calls `POST /v1/internal/referral/partner/:commerceUserId/team-invitations`
  with an already-resolved `inviteeCommerceUserId`, `overrideBps`, `teamOverrideMaxBps`,
  `teamInvitesEnabled`, and bounded B2B authority. The member's platform-funded direct rate is the
  Sales default (10% / 1000 bps), never an inviter input. Both edge and delegated ceiling are
  bounded by the inviter and the hard 20% maximum. `PATCH .../team/:memberCommerceUserId` changes
  those controls only for a direct active Commerce member;
  lowering a delegated Team ceiling atomically clamps dependent edges and pending invites
  leaf-first. B2B self-service and B2B delegation are separate rights. An inherited B2B grant can
  never exceed its direct parent, while a direct platform grant cannot be edited by that parent;
  narrowing an inherited source clamps or revokes its descendants and pending invites in the same
  transaction.
  A partner whose own Team-invitation right is disabled may still revoke that right from a direct
  member, but cannot grant/re-enable it on the member.
  Team rows expose immutable Commerce identity server-side, status, fixed direct rate, the exact
  share/ceiling, referral count, the member's net earnings and the inviter's exact Team net. Commerce
  enriches those UUIDs with current emails and removes UUIDs from public responses. Money remains
  decimal nanoUSD strings. Older `/v1/partner/*` writers remain expand-only compatibility until the
  Dashboard consumer is production-verified.
- Terms (bps) are individual per partner, set in the admin panel.
- Payouts: the partner binds a BSC wallet and sees automatic half-month periods. The operator
  prepares and sends a fenced immutable batch only inside the payout window; old manual requests
  are reject-only compatibility history.

Migration `packages/sales-db/migrations/0024_team_override_controls.sql` was deployed before this
Team consumer. It provides an admin-set partner ceiling
`team_override_max_bps` (hard range 0..2000 bps; NULL is the rollout/default 20% ceiling), an exact
child-edge `parent_override_bps`, and the same snapshot fields on invites. Existing NULL edges continue to use the deployed parent's
`sub_commission_bps`; every new Team invite writes an explicit edge and delegated ceiling. Cross-row
database guards repeat the API ceiling checks, and the v2 immutable commission trigger verifies the
same exact edge before accepting money.

Migration `packages/sales-db/migrations/0025_partner_authority_requests.sql` is the schema
authority for unified partner administration. It separates B2B self-service from the authority to
delegate B2B rights, records whether a child grant came directly from the platform or from its
parent, and makes unsafe parent-right narrowing fail closed until dependent grants and pending
invites are clamped in the same transaction. It also creates empty, immutable request/provider
decision tables and a durable idempotent Commerce-effect outbox for B2B conversion/pricing and
direct-commission-change reviews. The migration creates no grant, request or effect. The active
consumer exposes keyset-paginated partner/admin queues, requires a reason and a stable
`Idempotency-Key` (hashed under the authenticated partner before it reaches the global database
index), applies commission approvals inside Sales, and delivers approved B2B decisions through a
unique-attempt lease token plus Commerce `operationRef`. A stale worker cannot acknowledge a
reclaimed attempt; retryable failures use a bounded exponential delay, while request, ownership and
payload conflicts remain visible as terminal `apply_failed` evidence. A retryable failure keeps the
referral fenced from a second request; a closed terminal effect permits a new independently reviewed
request without mutating the historical failure.

Migration `packages/sales-db/migrations/0026_commerce_partner_membership.sql` was deployed first as
an expand-only cutover checkpoint. It adds a unique Commerce user identity and explicit program state
to partners, a Commerce user target to invitations, and calculation-version plus exact
`gross_amount_nano` / `withheld_amount_nano` evidence to both commission ledgers. Existing rows
remain version 1, no Telegram account/session/invite is changed, and no financial row is updated.
The active version-2 database guard requires every participating ancestor to be an enabled
Commerce membership, caps each Team edge at 20%, and verifies
`net = gross - withheld` independently of insert order. It excludes any participant whose
membership starts after the usage event, so manually admitted accounts begin with zero new-program
commission. The dependent Sales producer shipped only after that migration SHA was production-
GREEN. The Commerce consumer is a separate producer-first stage and remains forbidden from
production until the complete request-view producer described below is production-GREEN.

Migration `packages/sales-db/migrations/0027_terminal_commerce_invites.sql` was deployed next. Its
Team/B2B narrowing guards ignore an invitation only after `revoked_at` is committed, so current
authority can be reduced without rewriting terminal evidence. Consumed/revoked Commerce
invitations are database-immutable and Commerce invitation evidence cannot be deleted; legacy
invitation lifecycle is unchanged. The revocation/cascade producer below depends on that
production-GREEN checkpoint.

### Commerce → Sales membership boundary (`/v1/internal/referral/*`)

All routes require `x-api-key: SALES_CONTROL_KEY`, return `Cache-Control: no-store` on reads, and
accept Commerce UUIDs only from the server-side consumer. The browser never calls them directly.

- `POST membership/resolve {commerceUserId}` atomically returns `unavailable`, `disabled` or
  `active`. A valid account-bound invitation activates once, creates an enabled membership with
  `program_started_at=now()`, consumes the invitation and writes audit evidence. Exact retries
  return the same membership. An invalidated parent revokes the pending invite instead of creating
  an orphan.
- `GET partner/:commerceUserId?days=N` returns one Dashboard snapshot: safe membership authority,
  net/gross/adjustment totals, referrals, direct Team, account-bound invites, request history,
  payout periods and Usage-style aggregate/daily provider earnings. It contains no promo authority
  or Telegram product identity. Every request view on this internal boundary additively carries
  nullable `customerCommerceUserId`; Commerce uses it together with the other referenced account
  UUIDs to attach current authoritative emails, then removes all UUIDs and Sales partner ids before
  returning a browser response. The legacy partner/browser contracts do not expose this field.
- Partner mutations: `POST/DELETE team-invitations`, `PATCH team/:memberCommerceUserId`, `PATCH
  wallet`, `POST requests/commission`, `POST requests/b2b`, and `POST
  referrals/business-pricing`. Invitation revocation is owner-scoped, idempotent and cannot undo an
  activated membership. Wallet writes accept only a BSC `0x` address and commit the address plus
  audit evidence atomically under the active membership lock.
  Commission and B2B requests require a scoped `Idempotency-Key`; direct B2B writes require a grant,
  prove referral ownership, enforce the stored ceiling and then use the existing Commerce
  operation-ref boundary.
- Admin mutations: `POST admin/partners` immediately onboards/re-enables an account with all
  platform-owned terms; `PATCH admin/partners/:commerceUserId` edits them; admin list/request/payout
  routes expose only Commerce-linked program records. Disabled memberships remain visible in the
  request queue as immutable operational history, and the decision writer atomically rejects any
  legacy Telegram request id. A Commerce membership can be disabled but never physically deleted;
  every write carries the managed actor in `X-Admin-Actor` and is audited.

An admin onboarding wins over a pending Team invitation in one transaction and preserves the
original financial history/start timestamp on an existing membership. Team invitation creation is
serialized per invitee account: an exact retry returns the open invite, settings drift conflicts,
an expired invite is revoked before replacement, and an existing membership cannot be re-parented
by invitation.

Exact production-GREEN Sales producer SHA `a57b582b61a0d85cd42b90bdfce705611889fda9` adds the nullable
request identity field to every protected `/v1/internal/referral/*` request projection. The
Commerce consumer validates the field strictly; an older or malformed producer therefore fails
unavailable instead of dropping the customer email or exposing an internal identifier.

Commerce exposes the browser routes `/v1/referral/*` and `/v1/admin/referral/*`. It derives the
partner UUID from the verified session or an admin-side Commerce account lookup, resolves Team and
B2B targets from existing active account email, and removes every Commerce/partner/parent/grant-
source ID before returning data. Current Commerce email is the only account label on the unified
Dashboard/Admin. A failed profile lookup is explicit missing email data, not a UUID/Telegram/display-
name fallback. The server-only client uses a six-second timeout, strict Zod response schemas and no
automatic mutation retry.

The legacy Sales application decision remains an atomic compatibility path, not the unified
onboarding flow. An operator decision on an inbound application is one authority transaction, not a
create-then-patch workflow. `POST /v1/admin/applications/:id/decision` validates and writes the
new root partner's direct commission, default Team override, Team override ceiling, Team-invite
right, B2B self-service ceiling and B2B delegation right in the same Sales transaction that marks
the application approved. The audit row records those initial terms. A failed constraint therefore
leaves both the application and partner creation uncommitted; the UI must never claim an approved
account with partially applied authority. If another onboarding path already created the partner,
the application stays pending and the operator edits that existing partner directly; approval never
links an existing account while silently discarding the selected initial terms.

Partner request API:

- `POST /v1/partner/requests/commission` — request a higher direct commission with a reason;
- `POST /v1/partner/referrals/:userRef/b2b-requests` — request conversion/pricing for an owned
  Commerce referral; the request stores the immutable Commerce UUID but responses identify the
  customer by the fresh authoritative Commerce email;
- `GET /v1/partner/requests` — own keyset-paginated history;
- `GET /v1/admin/requests`, `GET /v1/admin/requests/:id`, and
  `POST /v1/admin/requests/:id/decision` — operator queue/detail/decision. A decision requires an
  `X-Admin-Actor` identity (legacy callers receive the explicit `legacy-sales-admin` fallback) and
  a non-empty note. Reject is terminal; commission approval is immediately `applied`; B2B approval
  is `approved` until the Commerce acknowledgement commits.

`GET /v1/partner/earnings/providers?days=N` returns both the aggregate `items` and additive `daily`
UTC points for the Usage-style stacked provider chart. Direct referral spend and downstream events
that actually paid a team override are included; gross earned parts reconcile with the commission
ledger for the same window. Historical rows with no provider remain under `providerId:null` instead
of being hidden or guessed. Manual signed adjustments have no provider evidence and stay outside
this descriptive split.

The active product UI has no promo-code entry point: the partner navigation/issuance screen, the
customer dashboard redemption screen and the old Sales admin controls are removed. Historical
promo ledger rows and the expand-only backend redemption/issuance contracts below remain only for
accounting evidence and rolling consumer retirement; their presence is not permission to restore a
product surface. Removing the now-dormant contracts is a separate producer-first cleanup after every
external consumer is proven retired; immutable financial and audit history remains retained.

The primary operator consumer is the managed `apps/admin` surface at
`admin.apitoken.sale/partners`, with child routes `/onboarding`, `/directory`, `/[id]`, `/requests`
and `/payouts`. Partner onboarding, directory/settings, request decisions and email-enriched payout
lists use the Commerce `/admin/referral/*` same-origin boundary; Commerce propagates the verified
operator identity to Sales under `SALES_CONTROL_KEY`, and the browser receives neither secret.
Fenced on-chain payout readiness/prepare/send/reconciliation continues through the existing guarded
`/partner-admin/*` routes until Sales adds the equivalent internal producer. Commerce must not
reimplement signing or payout calculations. `apps/sales-web` still serves its legacy
`/admin` only for route-by-route production parity verification. Removing or redirecting that legacy
surface is a separate release after the unified routes, mutations, RU/EN, light/dark, responsive
layout and payout fences have all been proven on the exact production SHA. Unified screens display
only current Commerce email or an explicit unavailable state; legacy-only identity fallbacks do not
cross the Commerce browser contract. Partner and unified Admin preferences use the main dashboard
keys `lang:v1` and `theme:v1`.

## Components

| Path | What | Port (dev) |
|---|---|---|
| `packages/sales-db` | its own PostgreSQL DB `sales` (Drizzle, own migrations, advisory-lock migrate) | — |
| `apps/sales-api` | NestJS/Fastify backend: auth, partner dashboard, admin panel, sync loop, email outbox | 3100 |
| `apps/sales-web` | Legacy Next.js partner/admin parity surface kept only through verified cutover | 3200 |

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
  Password and new-account OAuth registration insert this row inside the same PostgreSQL
  transaction as the user/profile/engine mapping, so a successful signup cannot lose its partner
  attribution after a transient second write. Promo redemption commits the same row before issuing
  its idempotent engine credit; an exact owner replay is idempotent, while a different existing
  first-touch owner returns 409 without credit. A storage failure leaves the request retryable
  instead of returning a credited-but-unattributed success.
  Response `{items:[{id,userId,code,createdAt}]}`; there is no `nextCursor` — the reader's cursor
  is the page's max `id` (rows come in ascending `id` order).
- `GET /v1/internal/sales/usage-events?after_id&limit` (default 1000, max 2000) — from
  `pricing_usage_events`; the cursor is the `feed_seq bigserial` column (migration 0012). Response
  `{items:[{id,userId,amountNano,providerId,accountClass,pricingMode,paidFundedNano,
  commissionEligible,snapshotDigest,occurredAt}], nextCursor,sourceHead}`; money fields are decimal
  strings. `sourceHead` is the latest committed `pricing_usage_events.feed_seq`, including rows
  still intentionally hidden by the visibility lag; it never authorizes cursor advancement, but
  irreversible payout work requires `nextCursor === sourceHead`.
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

    The consumer nevertheless **stores** the scalar `providerId`, in
    `partner_usage_events.spend_provider_id` (migration 0022) — a column separate from the retired
    attribution `provider_id`, which the legacy CHECK still binds to the whole tuple. The stored
    value is a reporting dimension with no authority: it is not compared during replay, a replay
    that disagrees leaves the recorded value untouched instead of failing the page, and a row that
    predates the column is enriched once. Commission is unchanged by it — identical spend earns
    identical commission whichever provider served it. Rows imported before 0022 keep
    `spend_provider_id` NULL and are reported as "no provider on record" rather than guessed, so a
    per-provider split still sums to the partner's recorded total.

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
- `GET /v1/internal/sales/topups?after_id&limit` (default 500, max 1000) — legacy rollback feed of
  paid `payments`. Its cursor is epoch microseconds from `paid_at`; the producer reads one look-ahead
  row and fails the page without cursor advance when equal `paid_at` timestamps cross the page
  boundary. The route remains unchanged while the Sales consumer rolls forward and back safely.
- `GET /v1/internal/sales/topups-v2?after_id&limit` (default 500, max 1000) — additive successor.
  `id`/`nextCursor` are `payments.feed_seq`, allocated only with the verified paid-row INSERT. The
  payment writer takes `SHARE ROW EXCLUSIVE` on `payments` before that INSERT: this serializes new
  transitions and fences an old rolling-deploy writer, making sequence allocation order equal
  commit order. Existing production rows are already paid-at-insert and replay from sequence zero.
  Every row in `payments` was created by a verified paid event. A later refund changes its current
  status but does not erase that historical deposit, so `topups-v2` deliberately does not filter on
  current payment status: rebuilding from cursor zero and an incremental consumer produce the same
  partner history. The producer limits the whole source stream before referral filtering, so
  unreferred rows advance the watermark; referred rows still require
  `paid_at >= attributed created_at`. Equal `paid_at` values are independently resumable. Response is
  `{items:[{id,paymentId,userId,amountNano,paidAt}], nextCursor,sourceHead}`. `sourceHead` is the
  latest committed `payments.feed_seq`, including a row still inside the visibility lag. Sales
  consumes this route under
  the independent `topups_v2` cursor from sequence zero; migration `0016_topups_v2_cursor.sql`
  reserved that key before the consumer shipped. The legacy timestamp cursor and route remain
  unchanged as rollback evidence, but are not the live health authority.
- `GET /v1/internal/sales/payment-reversals?after_id&limit` (default 500, max 1000) — additive
  producer for terminal refunds and provider-normalized chargebacks. The payment transition,
  engine-compensation intent and one immutable `audit_log(action='payment.reversed')` row commit in
  the same commerce transaction. Its shared audit `bigserial` is fenced against older writers;
  rows younger than 10 seconds stay hidden during rolling deploy. The page is limited before
  referral filtering and returns `{items:[{id,paymentId,userId,kind,amountNano,reversedAt}],
  nextCursor,sourceHead}`, so an ordinary customer's reversal advances the page watermark but does
  not cross the boundary. `sourceHead` is the latest committed `payment.reversed` audit id even
  while that row remains inside the visibility lag. Consumers never copy it into their cursor;
  irreversible work may instead require `nextCursor === sourceHead` and wait for visibility.
  `amountNano` is the exact original verified payment amount. This producer is
  deliberately deployable before any Sales schema/consumer; a missing consumer has no commerce or
  payout side effect.
- `POST /v1/internal/sales/referral-discount` — expand-only writer for the historical referral
  marker. Body `{userId, floorBps (0..9500), override?, actorId?}` →
  `{applied, multiplierBp:null, pricingAffected:false}`.
  It does not move any price: B2C pricing is the stored account scalar plus optional provider
  overrides, `multiplierBp` is always `null`, and no engine pricing job is enqueued. The marker is
  monotonic across automatic replay and can be explicitly replaced/cleared by the old override
  path. Only B2C profiles accept it. The route and columns remain for rolling compatibility and
  immutable audit evidence; the current partner/admin UI does not grant, edit or market them as a
  discount. Partner-facing additive responses that expose the old fields also return
  `pricingAffected:false`.
- `POST /v1/internal/sales/partner-business-pricing` — a granted partner prices **their own**
  referral as a B2B customer. Body
  `{operationRef?, userId, referralCode, ceilingPercent, discountPercent?, providers?, actorId?,
  reason?}` (whole percents; a
  provider mapped to `null` drops its override back to the customer's default) →
  `{operationRef, idempotentReplay, userId, converted, customerType, discountPercent, providers}`.
  `operationRef` is temporarily optional only for producer-first compatibility; the durable Sales
  effect consumer always sends its stable idempotency key plus the authenticated partner/admin
  actor. An exact replay returns the stored result, while reuse with any different canonical
  payload returns 409. Two guards, both required:
  the customer must be attributed to `referralCode` (first-touch attribution is the one ownership
  fact commerce can verify by itself), and every requested percent must be within
  `ceilingPercent`. The route is authenticated only as "sales", so the ownership proof is what
  keeps a defect on the sales side from repricing an unrelated customer; the ceiling is re-checked
  rather than trusted, and a disagreement fails closed instead of taking the more generous
  reading. Conversion requires an explicit base discount — provider overrides alone would leave
  the rest of the catalog at the B2C price. Conversion, the default, every provider override,
  their fenced `engine_pricing_jobs`, component audit and the terminal operation evidence commit
  in one Commerce transaction. The terminal `audit_log` fact is the durable replay ledger, guarded
  by the same transaction-advisory-lock pattern as idempotent admin credits; therefore a lost HTTP
  response cannot repeat jobs or audit. Provider ids are the closed `DISCOUNT_PROVIDER_IDS` set;
  an unknown id is rejected instead of being stored and never matching a request.

- `POST /v1/internal/sales/referral-profiles` — referral profiles for the partner's storefront.
  Body `{userIds: uuid[] (max 500)}` → `{items:[{userId, email, customerType (b2c/b2b), multiplierBp,
  discountPercent, referralFloorBps, cumulativeTopupNano, balanceNano, status}]}`. Only for an
  explicit list of user_id values that sales-api builds from the referrals assigned to the partner —
  a partner sees only their own. `email` is the authoritative Commerce account email and is not
  persisted in the Sales database; the producer joins it from `users` for this bounded request.
  `balanceNano` and the live `multiplierBp` are read from the engine
  (the money authority) with parallelism 8; an unavailable engine account does not take the page
  down — the fields degrade to `null`/values from `customer_profiles`.
  `referralFloorBps` is legacy audit/attribution metadata and `cumulativeTopupNano` is reporting;
  neither is used to calculate or display an applied customer price. `discountPercent` is the
  actual engine/commerce scalar discount.

Consumers on the sales side (`apps/sales-api`, reach commerce via `COMMERCE_BASE_URL`):

- `sync.service.ts` — sync loop over cursors (stored in the sales DB, interval `SYNC_INTERVAL_MS`,
  default 60 s). A tick is not one page: while any feed is still short of its committed head the
  loop keeps pulling, bounded by `SYNC_MAX_CATCHUP_PASSES` (default 50). One page per interval used
  to cap the money path at 1,000 billable events a minute — about 17 a second — so a busy hour left
  partner earnings hours behind even though payouts stayed correct through their own drain. The
  catch-up loop measured 1,269 events/s end to end against local PostgreSQL
  (`apps/sales-api/src/sync-financial-integrity.integration.test.ts`). The feeds themselves are
  unchanged: at-least-once delivery with idempotent writes, so a replayed page pays nothing twice.
  The order is attributions → assignment of the user to a partner + atomic replay of a legacy
  one-time marker claim (retained for old rows; it has no pricing effect);
  `topups-v2` → `referred_topups` (history/analytics only, create no commissions; replay starts at
  sequence zero and is idempotent by `commerce_payment_id`); usage events →
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
  timestamp equality. The three causal money feeds require explicit committed `sourceHead`; a head
  ahead of the visible cursor blocks payout rather than assuming absence. A 404 from the feed (the commerce side
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

Migration `packages/sales-db/migrations/0017_payment_reversal_accounting.sql` is the expand-only
schema checkpoint for the already deployed `payment-reversals` producer. It reserves and seeds
separate `topup_funding_lots` and `payment_reversals` cursors. The funding-lot cursor replays the
commit-ordered `topups-v2` source from zero without resetting its live analytics cursor, then stays
at the source head. The schema snapshots each referred topup as an immutable paid-funding lot,
and adds immutable allocation evidence from both usage stores to FIFO lots and from both commission
stores to exact lot-funded commission slices. One commerce reversal can name only its original
payment lot; every signed adjustment must be the exact negative slice funded by that lot. Database
guards enforce matching user/partner/source sums, causal timestamps, FIFO, complete bounded usage
allocation, deterministic integer rounding, SERIALIZABLE reversal writes, a deferred complete
adjustment-set check and immutable evidence. This checkpoint intentionally
does not consume the feed, backfill lots, change an earning or unblock/block a payout; the dependent
consumer and signed readers are delivered only after this migration SHA is green in production.

Migration `packages/sales-db/migrations/0018_reversal_completeness_fence.sql` is the final dormant
schema fence before that consumer. All writes touching one paid-funding lot serialize on its row;
reversal creation is `SERIALIZABLE` and fires a deferred completeness check even when it produces
zero adjustment rows. Deferred checks also reject a usage or commission slice added behind an
already committed reversal unless the same serializable transaction records every exact negative
slice. Locally known usage through `reversedAt` must be completely allocated first. The future
consumer additionally gates reversals on Commerce usage and funding-lot cursor catch-up: the Sales
database cannot prove absence of a source row that has not arrived over HTTP.

The deployed consumer now owns both reserved cursors. `topup_funding_lots` independently replays
the canonical `topups-v2` stream from zero and snapshots each already recorded referred topup;
analytics `topups_v2` remains untouched. A bounded reconciler walks scalar/v2 usage in immutable
event order, consumes causally available payment lots by `commerce_topup_id` FIFO, and creates every
commission slice with cumulative integer-floor rounding. A later source page cannot overtake an
incomplete earlier usage for the same user. After reading a non-empty reversal page and before
committing it, the consumer makes fresh usage and funding-lot requests; both must return no-advance
pages, proving they reached a visibility cutoff no earlier than the one that exposed the reversal.
Feed ids are independent sequences and are never compared.
`payment-reversals` then commits the reversal, every exact negative adjustment and its cursor in one
`SERIALIZABLE` transaction. Exact crash replay is a no-op; missing source evidence or any immutable
field conflict leaves the cursor behind. Earnings and payout readers now expose gross, signed
adjustments, net, debt and payable separately. Prepare/send actively drain all causal feeds to fresh
source heads and fail closed on cursor lag or incomplete allocation/reversal evidence. The sync
mutex remains held through a final read-only Commerce-head probe under the shared Sales accounting
lock; the final balance proof and commitment/signing remain inside that fence. Legacy manual payout
rows are reject-only, and an old batch without an `earned_before` checkpoint must be re-prepared.

### Sales → Commerce: registration and dormant promo compatibility (`apps/sales-api/src/internal.controller.ts`)

Commerce calls sales-api at `SALES_API_URL` with the same `SALES_CONTROL_KEY`.

- `POST /v1/internal/partners/external-referral-alias` — idempotently issues an opaque public
  `?ref=` alias for one trusted server-side acquisition object. Body
  `{source, externalRef, partnerCode}` → `{source, externalRef, code, partnerId, createdAt}`.
  `(source, externalRef)` is immutable: an exact replay returns the same alias, while an attempt to
  move it to another partner returns 409. `partnerCode` must belong to an active partner. The alias
  contains no email/contact identifier, resolves to that partner during the existing attribution
  sync, and always carries `discountBps=0`; it cannot create or revive a legacy price marker. This
  producer is consumed only by Commerce, under `SALES_CONTROL_KEY`; an external CRM never receives
  that broad boundary key.

- `POST /v1/internal/promo/redeem` — dormant legacy producer with no live Commerce consumer. The
  public `POST /v1/promo/redeem`, its controller and its service have been removed consumer-first.
  The internal route and stored rows remain temporarily so financial/redemption history is not
  rewritten; removing that producer is a separate final contract-removal stage. Its legacy body
  `{code, commerceUserId}` → `{valueNano, partnerId, referralCode, redemptionRef, discountBps,
  pricingAffected:false, alreadyRedeemed}`. Atomic and idempotent by (code, user): a repeat redemption by the same user
  returns the same `redemptionRef`, so the engine credit on the commerce side is idempotent by ref
  (retries are safe). One-time code; one promo per user (409); promo credit is available only to an
  unassigned account or as an exact-owner replay, and a different permanent first-touch attribution
  fails closed before engine credit. The code is unavailable if the
  partner is not active or the promo is disabled. The removed Commerce consumer historically
  credited the engine after durable owner attribution and stored a nonzero `discountBps` only as an
  audit marker; no current product route can trigger that workflow. It never changed the B2C
  scalar/provider price.
- `POST /v1/internal/partners/referral-discount` — atomic claim of a legacy one-time attribution
  link. Body `{code, commerceUserId}` → `{discountBps, pricingAffected:false}`. First-wins and
  idempotent by (code, user); an ordinary or already-consumed code returns zero. Called from
  `apps/api/src/auth.service.ts` at the first activation of the engine account (password
  registration, email confirmation, OAuth) as a best-effort compatibility replay; a failure is
  retried by the async attribution feed. It does not create or promise a personal price.
- `GET /v1/internal/partners/resolve?code` → `{found:false}` or `{found:true, partnerId,
  referralDiscountBps}` — resolving the ref code of an active partner (`Cache-Control: no-store`).
  The endpoint is live, but the current commerce code does **not** call it: the claim endpoint
  above replaced the resolve+consume pair, closing the window where one marker could be claimed by
  several registrations.

Rules: sales does not open the commerce/engine PostgreSQL and does not import `@claude-api/db`;
commerce symmetrically does not open the sales DB — everything goes through HTTP under the key.
Money amounts — only integer nanoUSD decimal strings. Referral email disclosure is limited to the
partner who owns that referral and managed Sales admins: Sales first resolves the owned user-id set,
then asks Commerce for profiles for exactly that set. The email remains authoritative in Commerce
and is not copied into Sales storage. Partner referral rows, managed-admin partner detail and
referral/deposit activity expose that email; if Commerce is unavailable they retain only the short
UUID mask for that response rather than caching or inventing an identity.

External referral aliases live in `external_referral_aliases`, not in
`partner_discount_links`. Their only job is identity-preserving attribution to an ordinary partner;
commission, suspension, first-touch user binding and every payout rule remain unchanged. Deleting a
partner with an issued alias is blocked just like deleting one with referral history.

## Attribution on the main site

`apps/web`: a valid `?ref=CODE` on any main-site page is saved to first-party localStorage for
30 days and sent at registration as `referralCode`. Initial capture runs before visible navigation;
client-side route capture repeats it, and locale changes preserve the full query and fragment. The
latest distinct referral click wins, while revisiting the same code does not extend its expiry.
Commerce commits the code to `referral_attributions` (unique by user_id) in the same transaction as
a new password/OAuth account. Ref is also passed through OAuth registration: the social buttons pass it in `oauthUrl` (`apps/web/src/lib/api.ts`),
`beginOAuth` saves the code in the OAuth transaction (it survives the redirect to the provider), and
`completeOAuth` for a **new** account writes the attribution. The current code also calls
`POST /v1/internal/partners/referral-discount` only to replay a legacy one-time marker. Ref affects
attribution and commission; it never changes or promises a price. B2C price remains the stored
account scalar plus any provider override.

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

The additive admin payout preview `GET /v1/admin/payout-list` includes `email: string | null` on
every due row. Operator UIs use it as the primary partner identity, then fall back to display name,
Telegram and the short opaque partner id only for legacy or temporarily unenriched accounts. The
email is display metadata only: eligibility, amount, wallet fencing and payout identity continue to
use the immutable `partnerId`.

Preparation revalidates amount/address under partner locks and pins the hot wallet; send/retry/poller
share a cross-process lock, persist exact hash/raw/nonce before broadcast and mark paid only from a
confirmed BSC receipt. `SALES_MIN_PAYOUT_USD` is also the execution threshold; the retired fractional
`PAYOUT_MIN_USD` knob does not exist.
The Sales Web send controls are an additional fail-closed safety layer: API money must be canonical
nanoUSD, row/recipient/batch/required totals must agree, both balances must be explicitly sufficient,
the current hot wallet must match the batch-pinned wallet, and no row may remain in `broadcast`.
Malformed money is rendered unavailable rather than as a false `$0.00`. The backend remains the
authority and repeats all irreversible checks under the cross-process send lock.

The additive read-only `GET /v1/admin/payouts/engine` projection reports configuration/window plus
`chain{ready,hotWalletAddress,usdtBalanceNano,bnbBalanceWei,gasCostPerTransferWei,issue}`. Balances
are canonical integer strings (`nanoUSD` for USDT and wei for BNB); a chain/RPC/token proof failure
returns null balances with `issue=read_unavailable`, never a fabricated zero or a provider error.
The private key and RPC endpoints never cross this boundary. Older consumers may ignore `chain`.

`GET /v1/admin/events` is the additive managed-admin SSE invalidation feed. A single listener in
the Sales API process consumes the commit-bound `sales_admin_changes` PostgreSQL channel and maps
each allowlisted table to partner, application or payout resource prefixes. Module initialization
awaits the single process-wide listener; browser streams only subscribe to its in-memory fanout,
so concurrent first streams cannot duplicate `LISTEN` or race their initial refetch. Every browser stream
gets an initial `resync`; reconnecting the database listener emits another owner-wide `resync`
because PostgreSQL `NOTIFY` is deliberately non-durable. Heartbeats only keep the transport open
and never cause a read. The route uses the same `x-sales-admin-key` boundary as the other admin
endpoints and returns no partner or money data itself.

## Commission math (sales-db)

For the live scalar usage event, `A` is `amountNano`, already narrowed by commerce to the
customer-funded `real_funded_nano` after free-first accounting and settlement shortfall. Historical
v1 rows use the same writer; historical v2 rows use their exact `paid_funded_nano`. For a user of
partner P0:
- direct gross `G0 = floor(A * P0.commission_bps / 10000)`;
- next parent cut `Wn = floor(Gn * edge(Pn→parent).parent_override_bps / 10000)`; only an older NULL
  edge falls back to the parent's `sub_commission_bps`;
- current net `Nn = Gn - Wn`; if the parent is eligible, `G(n+1) = Wn`;
- conservation: `Σ Nn = G0` exactly, including integer flooring;
- stop: no eligible parent, zero cut, or 10 entries (levels 0..9). Eligibility requires active
  status, `program_enabled=true` and `program_started_at <= event.occurred_at`.

New writes in both commission ledgers use `calculation_version=2` and store `gross_amount_nano`,
`withheld_amount_nano`, and net `amount_nano`. Entries are idempotent via the unique
`commerce_event_id`; calculation and usage insert share one transaction. The database trigger
reconstructs the chain and rejects invented partner/rate/gross/withheld/net evidence. Before
computing either store, every traversed partner row is held `FOR SHARE`, so membership/status/bps
cannot change under calculation; a cycle rolls back the event. Existing calculation-version 1 rows
remain immutable and readers sum both stores via `UNION ALL`.
Payout balance = `max(confirmed gross + signed adjustments − (paid + active requests), 0)`.
Debt = `max(paid − (lifetime gross + signed adjustments), 0)` and remains visible separately.

## Local tests

Affiliate money suites (`packages/sales-db` `*.integration.test.ts` and
`apps/sales-api` payout/sync SQL tests) run only when `TEST_SALES_DATABASE_URL`
is set. `pnpm test` without that variable exits 0 and skips them.

Local topology matches production and the watchdog: a `sales` database on the
`compose.yaml` Postgres, not a second container.

```bash
docker compose up -d
bash deploy/local-test-databases.sh ensure
TEST_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/commerce \
TEST_SALES_DATABASE_URL=postgresql://commerce:commerce-local-only@127.0.0.1:5433/sales \
  pnpm test:integration
```

`pnpm test:integration` fails closed if either DSN is missing. The local merge
TypeScript lane defaults those URLs, creates the databases, migrates, and then
fails closed if Postgres is down.

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
  in `RestrictAddressFamilies`, otherwise Next crashes on `uv_interface_addresses`. Next exits
  with status 143 after the rollout's normal SIGTERM, so the unit admits exactly 143 as a clean
  exit; removing that exception turns every planned replacement into a false systemd failure.
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
- Click statistics storefront (today we only count registrations and spend).
- Notifications to the partner (email/TG) about new referrals and accruals.
