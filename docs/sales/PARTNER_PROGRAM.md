# PARTNER_PROGRAM.md — apitoken.sale partner program (complete guide)

A complete description of the sales arm (`partners.apitoken.sale`): how it works from a
salesperson's sign-in to a payout arriving at their wallet, what a partner sees and what an
operator sees in the admin panel, and how everything is calculated. This is the "product"
documentation; the technical map of the bounded context is
`docs/sales/SALES_PORTAL.md`, the detail on payout periods is `docs/sales/SALES_PAYOUT_PERIODS.md`.

Contents:
1. What this is and who it is for
2. Sign-in and onboarding (Telegram only, invite-only, applications)
3. Referral links and attribution
4. Commission: on what and how much
5. Multi-level structure (sub-salespeople and override)
6. Wallet and payout currency
7. Periods, lock and payouts
8. Partner dashboard
9. Admin panel (admin.partners.apitoken.sale)
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
The program is **invite-only**: an account is created only by invitation or after an application
is approved.

A separate product with its own domain and database. Its cabinet uses the same card hierarchy,
responsive layout, light/dark theme and RU/EN preferences as the main dashboard:
- partner site — `https://partners.apitoken.sale`
- operator admin panel — `https://admin.partners.apitoken.sale`

## 2. Sign-in and onboarding

**Authentication — only via Telegram** (the official Telegram Login Widget). No passwords, no
email. Three scenarios on `/login` and `/register`:

- **Already have an account** (this `telegram_id` is already a partner) → straight into the
  dashboard.
- **There is an invitation to your @username** → press "Sign in with Telegram" and the account is
  created automatically (the invite is bound to the username; signing in with it confirms the
  match). A link of the form `partners.apitoken.sale/register?invite=CODE` is not required —
  a regular sign-in is enough.
- **No invitation** → you are offered to **submit an application** (write where you will promote).
  The application goes to the operator for review; after approval, the next Telegram sign-in
  already has the account ready.

Who issues invitations:
- **The operator** — in the admin panel (Onboarding tab) for any `@username`, with individual
  percentages. These are the "root" salespeople (no parent).
- **A partner** — in their own dashboard (Team tab) for a sub-salesperson's `@username`.

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

Example: a referral actually spent **$100** of their own money on the API → a partner with 10%
gets **$10**. If they spent the welcome bonus — the partner gets nothing from that portion (free
credit is spent first).

A salesperson's referrals keep their ordinary account pricing: the stored default (normally 50%
for B2C) plus any explicit provider overrides. A referral link controls attribution and commission,
not price. Historical `referral_*discount*` / `referral_floor_bps` fields are legacy attribution
markers retained for expand-only API and audit compatibility; the current partner/admin UI neither
grants nor presents them as a discount. They never enqueue a pricing job or change the engine
multiplier. B2B remains a separate model with its own negotiated default/provider terms.

## 5. Multi-level structure (sub-salespeople and override)

A partner can invite **sub-salespeople** and receive an **override** — a percentage of their
commission.

- A partner has a platform-set `commission_bps` (their own direct percentage on referrals), an
  admin-set `team_override_max_bps`, and one exact `parent_override_bps` on every child edge.
  `sub_commission_bps` remains only the fallback for older NULL edges.
- The chain is computed upward up to **10 levels**: level 0 is the direct referrer, level 1 is
  their parent (receives the exact child-edge percentage of the level-0 amount), and so on; it stops when
  there is no parent, the amount becomes 0, or the level exceeds 10. A blocked (`suspended`)
  partner breaks the chain.
- **Margin constraint:** the inviter cannot change the sub-salesperson's direct platform rate
  (10% by default). They choose only their own override and the maximum the member may delegate;
  both are bounded by the inviter's ceiling and the platform hard maximum 20%.

All accruals are idempotent by the charge's `commerce_event_id` and computed in the same
transaction as the usage-row insert. Live scalar and historical v1 rows use
`partner_usage_events`; `partner_usage_events_v2` remains only for historical release-v2 replay.

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

### What a granted partner can actually do

From **Referrals** in the cabinet, a granted partner gets one extra action per referral:
"Make B2B" for a B2C referral, "Edit rates" for one already converted. It opens a base discount
plus optional per-provider overrides (anthropic, openai, google, kimi, glm — the same closed list
commerce accepts, so a typo cannot be stored and then silently never match a request). A blank
provider field leaves that provider on the customer's base discount; clearing one drops it back.

Converting requires a base discount: provider overrides alone would leave every other model on the
ordinary B2C price. A partner without the grant sees none of this — the column does not render.

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

## 8. Partner dashboard (`partners.apitoken.sale`)

- **Overview** — the rate and what the percentage comes from (a "How your commission works" card
  with an example), the ref link with copy, key metrics, a 30-day chart, a 30-day split of earnings
  by the provider that served the referrals' requests (`GET /v1/partner/earnings/providers`), and
  recent referrals. The split includes stacked daily bars and aggregate provider totals. It only
  re-groups commission that is already recorded — it never changes what is owed, since the same
  spend earns the same commission on every provider. Spend recorded
  before the portal stored the provider (migration 0022) appears as one "no provider on record"
  line rather than being dropped, so the parts always sum to the whole.
- **Referrals** — the users brought in, identified by their authoritative Commerce account email,
  their paid spend and your earnings on each of them. If Commerce is unavailable for one response,
  the row falls back to a short UUID mask; Sales never persists or guesses the email.
- **Team** — create one-time, 30-day invitations for sub-salespeople, review invitation status, and
  see each direct member's referral count, direct earnings, and your override. The form shows the
  fixed 10% platform rate read-only and lets the inviter set an edge override plus a delegated
  ceiling within their own maximum. Existing direct-member controls are editable; lowering a
  ceiling clamps dependent grants atomically. The immutable commission chain carries each exact
  edge up to ten active levels.
  The dashboard uses the additive `/v1/partner/team/invites` writer; the previous writer remains
  only during the expand-only rollout and is retired after every documented consumer moves.
- Promo-code creation and redemption are absent from the active partner/customer/admin interfaces.
  Historical credit/accounting records remain readable by backend reconciliation only; they are
  not a product capability.
- **Payouts** — BSC wallet binding, the current period, the locked amount + unfreeze date, the
  date and estimate of the next payout, explicit debt after refunds, net history by periods, a
  "How payouts work" explanation.
- **Settings** — profile (display name) and commission terms (view only).

## 9. Admin panel (`admin.partners.apitoken.sale`)

A separate admin site; sign-in — the operator enters `SALES_ADMIN_KEY` (sent as
`x-sales-admin-key`). Tabs:
- **Overview** — summary (partners, users brought in, spend, commissions, payout queue).
- **Onboarding** — program applications (approve with percentages / reject) and issuing root
  invites to `@username`s.
- **Partners** — partners table: changing percentages, freezing/enabling, deletion (only without
  history).
- **Payout list** — the auto-generated "to be paid" list for the current/last period's window: who
  is ready for payout (wallet + amount > 0), who is held (no wallet), amounts and wallets.
- **Send payouts** — prepares one immutable batch, pins amounts/recipient addresses/hot wallet,
  first proves the usage/funding/reversal feeds and allocation ledger are current, checks BSC
  mainnet, canonical USDT, balances and gas, then sends sequentially with durable
  hash/raw/nonce evidence and receipt reconciliation. Irreversible sends are server-gated to the
  3-day payout window.

## 10. Interface languages

The partner dashboard works in **Russian and English**. The language is chosen with a switcher in
the header and remembered in the browser; by default it is taken from the browser language. The
light/dark theme uses the same `theme:v1` browser preference as the main dashboard and is toggled
from the header. A missing or invalid preference uses the same dark default as the main dashboard.
Every partner and Admin surface supports Russian and English, including the Admin sign-in gate;
both use the shared `lang:v1` and `theme:v1` preferences.

## 11. Privacy and security

- A partner receives only the authoritative email of users attributed to that partner. They never
  receive a full Commerce UUID; managed admins receive the same email through the bounded profile
  request. Sales does not persist it, and an outage falls back to `user-xxxxxxxx…` for that response.
- The partner side has no access to the commerce/engine DBs; the only connection is a read-only
  HTTP feed under a server key. From sales you cannot touch commerce money or the engine.
- Telegram sign-in is verified via the bot's HMAC signature; sessions are stored hashed; the admin
  key lives only on the server. A security audit has been completed (see history; nothing critical).

## 12. What is not done yet

- Separate email/Telegram notifications for payout confirmation. The partner already sees the
  exact amount, receipt status, transaction hash and BscScan link in the dashboard.
