# PARTNER_PROGRAM.md — apitoken.sale partner program (complete guide)

A complete description of the partner program: how it works from a Commerce-account membership
to a payout arriving at the partner's wallet, what a partner sees and what an
operator sees in the admin panel, and how everything is calculated. This is the "product"
documentation; the technical map of the bounded context is
`docs/sales/SALES_PORTAL.md`, the detail on payout periods is `docs/sales/SALES_PAYOUT_PERIODS.md`.

Contents:
1. What this is and who it is for
2. Commerce-account membership and onboarding
3. Referral links and attribution
4. Commission: on what and how much
5. Multi-level Team and retained share
6. Wallet and payout currency
7. Periods, lock and payouts
8. Partner dashboard
9. Admin panel (admin.apitoken.sale)
10. Interface languages
11. Privacy and security
12. What is not done yet

---

## 1. What this is and who it is for

An affiliate (referral) program for **salespeople** who bring customers to apitoken.sale.
A partner earns **a percentage of what their referrals ACTUALLY SPEND** on API usage (the amount
charged, accounting for their discount). Commission — ONLY on the collected portion covered by real
customer money: free credit (welcome bonus, promo, admin credit) is spent first and earns no
commission; settlement shortfall funded by the pool earns none either.
The program is **invite-only**: membership is enabled either immediately by an operator for an
existing Commerce account or by an account-bound Team invitation. There is no public self-service
application in the new Dashboard flow.

Sales remains a separate financial bounded context and database, but it is not a separate user
identity or long-term product surface. The canonical partner cabinet is the **Referral** section of
the authenticated Commerce Dashboard; the canonical operator surface is the main Admin panel. Both
use Commerce account UUID as the immutable membership key and display the current Commerce email.

The rollout is producer-first. While the Dashboard/Admin consumers are being deployed and checked,
`partners.apitoken.sale` and `admin.partners.apitoken.sale` remain available only as legacy parity
surfaces. They are retired in a later release; disabling them before the replacement is production-
verified would remove the only working access path. Historical Telegram identity, sessions,
commission, reversal, payout and audit rows are retained, but legacy partners have
`program_enabled=false` and receive no new version-2 accruals.

## 2. Commerce-account membership and onboarding

There is no second login. An authenticated apitoken.sale account opens Dashboard → Referral, and
Commerce calls Sales under `SALES_CONTROL_KEY` with the session-derived `users.id`. The browser is
never trusted to choose a different membership UUID.

Three states are explicit:

- **active** — the Commerce account has an enabled partner membership and receives the full cabinet;
- **disabled** — the membership exists, but an operator disabled the program or suspended the
  partner; financial history remains intact and no new commission is created;
- **unavailable** — the account is not a partner and has no valid invitation. The Dashboard shows
  the standard terms and a button to contact `https://t.me/bozinodev`; it does not silently create
  access or accept a public application.

An operator can make any existing Commerce user a root partner immediately from the Users list or
partner onboarding screen. The same transaction sets their direct platform commission, Team-share
ceiling, Team invitation permission, B2B self-service ceiling and B2B delegation permission. An
open Team invitation for that account is revoked with audit evidence, so one account cannot belong
to two competing trees.

An enabled partner invites a direct Team member **by the email of an existing Commerce account**.
Commerce resolves the normalized email to one UUID before calling Sales; Sales stores only that
UUID, never an email snapshot. The invitation is account-bound, expires after 30 days and activates
idempotently on the invited user's next Referral-page access. Exact create/activate retries are
no-ops; a different open invitation or an existing membership is a conflict. The inviter chooses
the edge share and bounded delegated authority, but cannot choose the invited member's platform
direct rate (10% by default).

## 3. Referral links and attribution

Every partner has a **ref code** and a link to the main site:
`https://apitoken.sale/?ref=CODE`. When a person follows it, the valid code is remembered in the
browser for **30 days**; the latest distinct referral click wins, while revisiting the same code does
not extend its expiry. Navigation, including language changes, preserves the query until the code is
captured. Registration on apitoken.sale then binds that user to the partner **forever**. Attribution
happens only for registration via the link; it does not affect the user's own price/discount. Both
password registration and a new-account Google/GitHub OAuth callback carry the saved code. An OAuth
login to an existing account cannot attribute it or consume a legacy one-time referral marker.

## 4. Commission: on what and how much

A partner earns **a percentage of what a referral actually SPENDS** on API usage — of the amount
already charged under the account's default/provider multiplier. The immutable usage row records
the customer-funded basis, so a later discount change does not recompute history.

- **Real money only.** Free credit (welcome bonus: historical $4, new $5; promo; admin credit) **is spent
  first**: commerce
  maintains a "free balance" and for every charge computes `real_funded_nano` — the portion covered
  by real money. Referral commission comes **only from `real_funded_nano`**; the portion paid with free
  credit is zero.
- The source is the commerce usage feed (charges), where live `amountNano` is already the
  `real_funded_nano` result of free-first bucketing over the amount actually collected from the
  customer. This live scalar rule is identical for referred B2C and B2B accounts: negotiated B2B
  pricing changes the amount charged, not whether externally funded collected spend earns
  commission. Pool-funded settlement shortfall is excluded before the feed.
- Commission = `commission_bps × real_funded_nano` (integer, floor). `commission_bps` is the partner's
  rate (`1000 bps = 10%`), set by the operator.

Example: a referral actually spent **$100** of their own money on the API → the fixed 10% direct
commission pool is **$10**. With no parent, the direct partner gets all $10. With a 20% Team edge,
the direct member gets $8 and the parent gets $2; total platform payout remains exactly $10. If the
referral spent the welcome bonus, no commission is created for that portion.

A salesperson's referrals keep their ordinary account pricing: the stored default (normally 50%
for B2C) plus any explicit provider overrides. A referral link controls attribution and commission,
not price. Historical `referral_*discount*` / `referral_floor_bps` fields are legacy attribution
markers retained for expand-only API and audit compatibility; the current partner/admin UI neither
grants nor presents them as a discount. They never enqueue a pricing job or change the engine
multiplier. B2B remains a separate model with its own negotiated default/provider terms.

## 5. Multi-level Team and retained share

A partner can invite **Team members** and retain a **Team share** from the member's fixed platform
commission. It is a withholding inside that commission, not an additive platform bonus.

- A partner has a platform-set `commission_bps` (their own direct percentage on referrals), an
  admin-set `team_override_max_bps`, and one exact `parent_override_bps` on every child edge.
  `sub_commission_bps` remains only the fallback for older NULL edges.
- The direct gross pool is `customer-funded spend × direct commission`. At each level the next
  parent cut is `current gross × child.parent_override_bps`; the current member's net is
  `current gross − parent cut`, and only that cut becomes the next level's gross. Thus the sum of
  every net entry is exactly the original direct gross pool.
- The chain is computed upward for at most **10 entries** (levels 0..9). It stops at a missing,
  suspended, disabled, non-Commerce or not-yet-started membership, on a zero cut, or at the bound.
  An ineligible parent receives nothing and causes no withholding from the current member.
- **Margin constraint:** the inviter cannot change the sub-salesperson's direct platform rate
  (10% by default). They choose only the Team share they retain and the maximum the member may delegate;
  both are bounded by the inviter's ceiling and the platform hard maximum 20%.
- The platform can independently disable new Team invitations for a partner. B2B self-service and
  the ability to delegate B2B rights are separate settings; a delegated child ceiling cannot exceed
  the parent's ceiling. A partner with Team invitations disabled cannot grant that capability to a
  child. Platform-authored B2B rights remain outside the parent's authority.
- A partner who needs a higher direct commission submits a reasoned request. The parent cannot
  change that rate; only an operator decision updates it, atomically and without recalculating any
  historical commission.

All new-program accruals use `calculation_version=2`, store gross/withheld/net on each ledger row,
and are idempotent by the charge's `commerce_event_id` in the same transaction as the usage insert.
Database triggers independently reconstruct the active membership chain, timestamp gate and exact
integer-floor amounts. Version-1 rows remain immutable history. Live scalar rows use
`partner_usage_events`; `partner_usage_events_v2` remains for historical release-v2 replay.

If a customer's paid top-up is later refunded or disputed, the original positive commission stays
immutable for audit and an exact negative adjustment is appended for the slices funded by that
payment. This reduces unpaid earnings immediately. If that commission was already paid, the partner
sees an explicit debt; future commission is withheld until the debt is repaid. No automatic debit is
made from the partner's external wallet.

## 6. Wallet and payout currency

Payouts — **only in USDT (BEP-20) on the BNB Smart Chain (BSC) network**. The partner binds a
BSC address in the dashboard (`0x` + 40 hex); the address is validated both at binding time and at
payout time. All payouts go strictly to the bound address; payment details in a request are not
accepted. Without a bound wallet the earnings are not lost — they accumulate and will be sent as
soon as a wallet appears.

## 7. Periods, lock and payouts

Accruals are counted over **two half-month periods**, i.e. **2 payouts per month**:

| Period | Days |
|---|---|
| P1 | 1 – 15 |
| P2 | 16 – last day of the month (30/31, February 28/29) |

Lifecycle of each period: accrual → **7-day lock** after the end of the period → **3-day payout
window**. In other words, the money for a period goes out roughly on the 8th–10th day after it
closes.

Key rule (it gives the lock, refunds and auto-rollover):
**net = gross commissions + signed refund adjustments; payable = max(net − committed payouts, 0).**
Already-paid commission above net is shown separately as partner debt.
- **Any amount above zero** is paid out — there is no minimum threshold.
- If a payout was not made (no wallet) — the amount is not lost, it rolls into the next window
  (it may arrive covering two periods at once).

In detail (phases, formula, time zone, what the partner/admin sees) —
`docs/sales/SALES_PAYOUT_PERIODS.md`.

## 7a. B2B grant (off by default)

By default a partner's link does one thing: the person who follows it becomes an ordinary **B2C**
customer on the global discount, and the partner earns their commission on what that customer
actually pays. No partner can change a customer's pricing.

An admin may grant a specific partner the right to turn **their own** referrals into B2B customers,
together with a **ceiling** — the deepest discount that partner may give. Both live on the partner
row (`b2b_enabled`, `b2b_max_discount_bps`, migration 0023) and can be set two ways:

- **at onboarding**, via the "B2B max discount %" field on the invite — the partner created from
  that invite already holds the grant;
- **later**, from the partner card in the partners admin ("B2B: off" / "B2B: up to N%").

The right and the ceiling are inseparable. Revoking the grant zeroes the ceiling in the same
statement, and an invite that does not grant the right stores no ceiling: a leftover number would
read as authority the partner does not have. The maximum any ceiling may reach is 95%, matching the
pricing policy range. Every change is written to `sales_audit_log` — giving away margin is a
decision that must be reconstructable, not just observable in the current row.

Every partner may also request B2B conversion or new pricing for one of their own referrals even
without self-service authority. Sales proves ownership by the immutable Commerce UUID, records the
requested default/provider terms and reason, and shows the customer by the fresh Commerce account
email. The operator may approve smaller/equal terms or reject with a mandatory note. Approval does
not claim success early: one durable effect retries Commerce with the same `operationRef`, and the
request becomes `applied` only after Commerce returns the matching acknowledgement. A payload-drift
409 is terminal and visible; a transport timeout is retryable and an exact Commerce replay cannot
duplicate pricing jobs or audit evidence. Retryable delivery keeps one active request for the
referral; after a terminal failure the partner may submit a new request, while the original decision
and failure remain immutable.

### What a granted partner can actually do

From **Referrals** in the cabinet, a granted partner gets one extra action per referral:
"Make B2B" for a B2C referral, "Edit rates" for one already converted. It opens a base discount
plus optional per-provider overrides. The active customer UI shows the production catalog only:
Anthropic/Claude, OpenAI/GPT, Google/Gemini and Kimi. GLM remains accepted by the expand-only
backend contract for historical/admin records but is not advertised in the partner Dashboard until
it is a production provider. A blank provider field leaves that provider on the customer's base
discount; clearing one drops it back.

Converting requires a base discount: provider overrides alone would leave every other model on the
ordinary B2C price. Every partner sees the B2B action for an owned referral. A partner with a direct
self-service grant applies terms immediately within the delegated ceiling; a partner without that
grant submits the same base/provider proposal and a required reason for operator review.

The ceiling is enforced **twice, server-side, and never read from the request**:

1. `apps/sales-api` checks the grant exists and that every requested percent — the base and each
   provider override — is within `b2b_max_discount_bps` from the partner row;
2. `apps/api` re-checks the ceiling AND proves the customer is attributed to the calling partner's
   referral code before writing anything.

The second check is not redundancy for its own sake: `/v1/internal/sales/partner-business-pricing`
is authenticated only as "sales", so without an independent ownership proof a defect on the sales
side would be enough to reprice any customer in the system. Every durable request effect carries a
stable `operationRef` and the authenticated actor. Commerce serializes that ref, rejects payload
drift, and commits the B2B conversion/default, `customer_provider_discounts`, durable
`engine_pricing_jobs`, component audit and terminal replay evidence together. A timeout after
commit can therefore return the stored output without repricing the customer or duplicating audit.

Commission does not change with the customer's class. A referred B2B customer earns the partner the
same percentage of the customer's own money as a referred B2C customer; a deeper discount simply
means the customer pays less, so the commission is smaller in absolute terms. Converting a customer
to B2B — whether by the central admin or under a partner grant — leaves attribution, commission and
the partner's referral list intact; the referral simply shows a `B2B` badge and its negotiated
discount.

## 8. Partner dashboard (Commerce Dashboard → Referral)

- The five compact subviews are **Overview**, **Referrals**, **Team**, **Payouts** and **Docs**.
  There is no separate partner Settings page and no Requests subview; account identity and
  theme/language remain owned by the main Dashboard. The workspace reproduces the partner cabinet
  (partners.apitoken.sale) layout: uppercase page and card titles, one joined stat strip, the
  commission formula, the referral link with copy, bullet explainers and bordered tables.
- **Overview** — the partner cabinet landing page: four key metrics, the commission formula, the
  referral link, the daily earnings chart and a 30-day split of earnings by the provider that served the
  referrals' requests (the provider series in
  `GET /v1/internal/referral/partner/:commerceUserId`), and
  a stacked daily chart. Provider cards and the graph intentionally reuse the main Usage geometry,
  logo registry, colors, responsive behavior and light/dark tokens. The active cards show Claude,
  GPT, Gemini and Kimi; GLM is not presented as production. The split only re-groups commission
  that is already recorded — it never changes what is owed, since the same spend earns the same
  commission on every provider. Spend recorded
  before the portal stored the provider (migration 0022) appears as one "no provider on record"
  line rather than being dropped, so the parts always sum to the whole. The longer commission/Team
  explanation lives in Docs rather than occupying the Overview.
- **Referrals** — the users brought in, identified by their authoritative Commerce account email,
  their type, discount, top-ups, paid spend and partner earnings. The list has client-side email
  search and exposes conversion/pricing on the owned referral row. That dialog always shows the
  partner's applicable ceiling, the production provider logos and base/per-provider fields; a
  self-service grant applies the terms directly, otherwise the same row submits a reviewed request
  with a required reason. If Commerce cannot resolve one account for a response, the row says that
  email data is unavailable; the Commerce Dashboard never substitutes a UUID, Telegram handle or
  display name. Sales never persists or guesses the email.
- **Team** — an invitation is an account email plus the share you retain from that member's own
  platform commission; every partner may build a Team, so that is no longer a permission and the
  invitation form carries no permission controls. The retained share is capped by the platform hard
  maximum of 20% and by the inviter's own maximum. The tab opens with what the Team produced
  (retained earnings, member earnings, referrals brought in, active members) and states the split as
  a formula: member commission × your retained share, never on top of it. The only per-member
  setting is B2B — whether that member may set B2B terms and the ceiling they may use, never above
  the owner's own ceiling; delegation follows the owner's own delegation grant. Lowering a ceiling
  clamps dependent grants atomically, the immutable commission chain carries each exact edge up to
  ten active levels, and a pending invitation can be revoked by its owner until it is accepted.
  The Dashboard uses the Commerce `/v1/referral/*` boundary, which calls Sales
  `/v1/internal/referral/*`; the browser never receives `SALES_CONTROL_KEY` or a Sales partner id as
  proof of identity.
- There is no Requests subview. A partner without a self-service B2B grant still submits a reviewed
  B2B request from the owned referral row; the request is decided by an administrator and its effect
  is delivered by the same durable pipeline. Commission-change requests are handled outside the
  Dashboard.
- Promo-code creation and redemption are absent from the active partner/customer/admin interfaces.
  Historical credit/accounting records remain readable by backend reconciliation only; they are
  not a product capability.
- An account without partner access sees an invitation instead of the workspace: the standard terms
  stated large (10% commission, the 20% Team ceiling, USDT BEP-20 payouts, two payout runs a month),
  the same commission formula, and an application form. No partner numbers, tabs or actions are
  rendered for that account.
- **Access applications.** The account submits one application with a short description of its
  traffic (`POST /v1/referral/applications`, session-owned; `GET /v1/referral/applications/me`
  returns its own latest one). One open application per account: submitting again refreshes the
  pending row rather than queueing a second review. The Dashboard then shows the pending or declined
  state with the reviewer's note, and @bozinodev in Telegram stays available in both states.
- Administrators work the queue in **Admin → Partners → Access applications**
  (`GET /v1/admin/referral/applications`, `POST /v1/admin/referral/applications/:id/decision`).
  Approving runs the same `onboardByEmail` path as manual onboarding — commission, Team ceiling and
  B2B ceiling are set in the decision dialog and default to 10% / 20% / B2B off — and the decision is
  recorded only after onboarding succeeds, so a failed Sales call leaves the application pending
  instead of marking an account approved that never got access. Rejecting records the note and never
  touches onboarding. A decided application cannot be decided twice. Storage is commerce
  `referral_applications` (migration `0050_referral_applications`); it records the review only,
  never partner terms, which stay authoritative in Sales.
- **Docs** — the partner cabinet documentation, numbered and personalised with the partner's own
  rate, Team ceiling, B2B ceiling and minimum payout; section 3 embeds the same commission formula
  shown on Overview.
- **Payouts** — the multi-lane accrual → 7-day lock → 3-day payout roadmap, completed on-chain
  payments, four current-state KPI cards, BSC wallet binding, explicit debt after refunds, net
  history by periods and a "How payouts work" explanation. The snapshot also returns the minimum
  payout and fixed
  lock/window policy, while the wallet writer is restricted to the active session-derived
  membership and commits every accepted change with its audit evidence in one transaction.
- **Docs** — the in-product contract for eligible paid usage, email identity, commission/refund
  math, retained Team share, B2B authority, wallet/network, payout schedule and access/privacy. It is
  personalized with the current platform commission, Team ceiling, B2B ceiling and payout minimum.

## 9. Partner administration

The primary operator surface is `https://admin.apitoken.sale/partners`. It calls the Commerce
admin boundary, which proxies the guarded Sales internal contract without exposing a Sales key to
the browser and propagates the authenticated managed operator as `X-Admin-Actor`. The routes are:

- `/partners` — overview, payout readiness and current partner analytics;
- `/partners/onboarding` — immediate Commerce-account onboarding by email with all initial
  authority boundaries set atomically;
- `/partners/directory` — email-first searchable directory with status, direct rate, Team ceiling,
  B2B authority and operational balances;
- `/partners/[id]` — one partner's direct commission, fixed Team defaults/ceiling, B2B rights,
  team, referrals and activity. Direct commission is platform-owned; a parent never edits it;
- `/partners/requests` — commission/B2B review queue with partner and customer email, requested and
  approved provider terms, mandatory notes, effect attempts and terminal/retry state;
- `/partners/payouts` — prepares one immutable batch, pins amounts/recipient addresses/hot wallet,
  proves the usage/funding/reversal feeds and allocation ledger are current, checks BSC mainnet,
  canonical USDT, balances and gas, then sends with durable hash/raw/nonce evidence and receipt
  reconciliation. Irreversible sends remain server-gated to the 3-day payout window.

Partner, Team, referral, request and payout identities on the unified surfaces are displayed only by
current Commerce account email. If that projection is unavailable, the row says the email is
unavailable and exposes no internal identity. Display-name, Telegram and short-mask fallbacks exist
only on the legacy Sales surfaces during cutover. Promo-code controls are absent. This does not
delete historical promo evidence or expand-only backend compatibility contracts.

The Commerce admin boundary owns partner onboarding, directory/settings, request decisions and the
email-enriched payout list. The existing `/partner-admin/*` on-chain readiness, prepare, send and
receipt-reconciliation endpoints remain the sole execution authority until an additive internal
Sales producer exposes the same fenced state machine to Commerce. The unified Admin may consume
that execution surface during the transition, but it must not duplicate the payout calculation or
signing logic in Commerce. Retiring `/partner-admin/*` is therefore a later producer-first change,
not part of the identity/UI cutover.

`admin.partners.apitoken.sale` remains online only as the legacy parity surface during rollout. It
must not be disabled, redirected or deleted until every route above has been verified in production
for RU/EN, light/dark, desktop/mobile, all mutations and payout safety against the exact deployed
SHA. Retirement is a separate release after that evidence exists.

## 10. Interface languages

The partner dashboard works in **Russian and English**. The language is chosen with a switcher in
the header and remembered in the browser; by default it is taken from the browser language. The
light/dark theme uses the same `theme:v1` browser preference as the main dashboard and is toggled
from the header. A missing or invalid preference uses the same dark default as the main dashboard.
Every partner-program surface in the partner dashboard and unified Admin supports Russian and
English, including the managed Admin shell; both use the shared `lang:v1` and `theme:v1`
preferences.

## 11. Privacy and security

- A partner's browser receives only the authoritative email of accounts attributed to that
  partner. Full Commerce UUIDs cross only the server-to-server Commerce↔Sales boundary, where they
  are required for ownership and membership checks; Commerce removes them from the public view.
  Sales does not persist email, and an unavailable Commerce profile remains an explicit missing
  email instead of falling back to an opaque identifier.
- Neither side opens the other's database. Commerce owns sessions/email; Sales owns membership,
  attribution and money ledgers. Every cross-context call uses `SALES_CONTROL_KEY` server-side.
- Legacy Telegram HMAC login and hashed Sales sessions remain enabled only during cutover. They do
  not grant `program_enabled` to legacy rows and are removed from routing only after the Commerce
  replacement passes production parity. Keys never reach either browser surface.

## 12. What is not done yet

- Separate email/Telegram notifications for payout confirmation. The partner already sees the
  exact amount, receipt status, transaction hash and BscScan link in the dashboard.
