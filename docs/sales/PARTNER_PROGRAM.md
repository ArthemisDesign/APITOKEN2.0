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

A separate product with its own domain, its own database and its own design (in the style of the
main site):
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
happens only for registration via the link; it does not affect the user's own price/discount.

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
  customer. Pool-funded settlement shortfall is excluded before the feed.
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

- A partner has two rates: `commission_bps` (their own direct percentage on referrals) and
  `sub_commission_bps` (override on sub-salespeople).
- The chain is computed upward up to **10 levels**: level 0 is the direct referrer, level 1 is
  their parent (receives `sub_commission_bps` of the level-0 amount), and so on; it stops when
  there is no parent, the amount becomes 0, or the level exceeds 10. A blocked (`suspended`)
  partner breaks the chain.
- **Margin constraint:** a partner cannot issue a sub-salesperson a rate higher than their own
  (otherwise they would be giving away the platform's margin). By default a sub-salesperson gets
  the parent's rate.

All accruals are idempotent by the charge's `commerce_event_id` and computed in the same
transaction as the usage-row insert. Live scalar and historical v1 rows use
`partner_usage_events`; `partner_usage_events_v2` remains only for historical release-v2 replay.

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

Key rule (it gives both the lock and the auto-rollover):
**payable = confirmed commissions up to the end of the period − already paid.**
- **Any amount above zero** is paid out — there is no minimum threshold.
- If a payout was not made (no wallet) — the amount is not lost, it rolls into the next window
  (it may arrive covering two periods at once).

In detail (phases, formula, time zone, what the partner/admin sees) —
`docs/sales/SALES_PAYOUT_PERIODS.md`.

## 8. Partner dashboard (`partners.apitoken.sale`)

- **Overview** — the rate and what the percentage comes from (a "How your commission works" card
  with an example), the ref link with copy, key metrics, a 30-day chart, recent referrals.
- **Referrals** — the users brought in (identities masked), their paid spend and your earnings on
  each of them.
- **Team** — sub-salespeople and invitations (the tab is currently temporarily closed with a
  "Soon" placeholder).
- **Payouts** — BSC wallet binding, the current period, the locked amount + unfreeze date, the
  date and estimate of the next payout, history by periods, a "How payouts work" explanation.
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
  checks BSC mainnet, canonical USDT, balances and gas, then sends sequentially with durable
  hash/raw/nonce evidence and receipt reconciliation. Irreversible sends are server-gated to the
  3-day payout window.

## 10. Interface languages

The partner dashboard works in **Russian and English**. The language is chosen with a switcher in
the header and remembered in the browser; by default it is taken from the browser language. The
admin panel is in English.

## 11. Privacy and security

- End users' identities are **not disclosed** to the partner — the dashboard shows only a masked
  `user-xxxxxxxx…`, without email or the full id.
- The partner side has no access to the commerce/engine DBs; the only connection is a read-only
  HTTP feed under a server key. From sales you cannot touch commerce money or the engine.
- Telegram sign-in is verified via the bot's HMAC signature; sessions are stored hashed; the admin
  key lives only on the server. A security audit has been completed (see history; nothing critical).

## 12. What is not done yet

- Separate email/Telegram notifications for payout confirmation. The partner already sees the
  exact amount, receipt status, transaction hash and BscScan link in the dashboard.
- Ref attribution through OAuth registration (currently only password-based on the main site).
