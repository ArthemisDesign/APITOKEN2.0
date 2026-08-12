# SALES_PAYOUT_PERIODS.md — partner program periods and payouts

How accruals and payouts to salespeople work over half-month periods: how to compute earnings,
when they get "frozen", when they are sent to the wallet, and how rollover works. The model is
deliberately built so that **all calculations are derived from already existing data**
(`commission_entries` + `commission_entries_v2` — a dual-schema UNION, the events do not overlap —
plus signed `partner_commission_adjustments` and `payouts`) without a separate "accruing" process — the period is defined by time and the
amounts are computed by a query on the fly.

Code: `packages/sales-db/src/periods.ts` (pure period math, covered by tests
`periods.test.ts`) and `packages/sales-db/src/payout-periods.ts` (state/history/list queries).
API — `apps/sales-api` (`GET /v1/partner/periods`, `GET /v1/admin/payout-list`). UI — the Payouts
tab in the dashboard and the "Payout list" tab in the admin panel.

## 1. Periods (half-month, UTC)

**Two periods in every month → 2 payouts per month:**

| Period | Days (inclusive) | Examples |
|---|---|---|
| **P1** | **1 – 15** | always 1–15 |
| **P2** | **16 – last day of the month** | July 16–**31**, April 16–**30**, February 16–**28/29** |

Everything is computed in **UTC** (deterministic; the time zone can be moved into config later).
The period key is `YYYY-MM-P1` / `YYYY-MM-P2` (e.g. `2026-07-P2`).

Technically, inside the code, boundaries are stored as half-open intervals `[start, end)` with an
**exclusive** end: P1 = `[1st 00:00, 16th 00:00)`, P2 = `[16th 00:00, 1st of next month 00:00)`.
This is the very same range — the 1st of the next month does **not** belong to the period, it is
already the start of the next P1. December P2 → January P1 of the next year is handled correctly;
the last day of the month (28/29/30/31) always falls into P2.

## 2. Period lifecycle

Every period goes through 4 phases (the `phaseOf(period, now)` function):

| Phase | When | What it means |
|---|---|---|
| `accruing` | `now < end` | Accrual is underway: new commissions fall into this period. |
| `locked` | `end ≤ now < end+7d` | **7-day lock**: earnings are finalized (a buffer for refunds/reconciliation). No payout yet. |
| `payable` | `end+7d ≤ now < end+10d` | **3-day payout window**: the "to be paid" list is formed, money goes to the wallet. |
| `closed` | `now ≥ end+10d` | The window has passed. If anything was not paid — it is **not lost**, it rolls into the next window (rollover). |

Constants: lock `LOCK_DAYS = 7`, window `WINDOW_DAYS = 3`.

### Timeline using P1 (July) as an example, ends July 16 00:00 UTC

```
Jul 1 ─ accruing ─ Jul 16 ─── lock 7d ─── Jul 23 ── window 3d ── Jul 26 ─ closed
                   (end)               (payouts            (window
                                         open)              closes)
```

Accordingly, July P2 (16–31, end = Aug 1) → lock Aug 1–8 → window Aug 8–11. In total, payouts
happen **twice a month**, roughly on the 8th–11th day after the period closes.

## 3. How much is payable: the formula and rollover

The main invariant that makes everything simple and gives rollover automatically:

> **Net** = immutable gross commissions + signed refund/dispute adjustments.
>
> **Payable to a partner in period P's window** = `max(gross commissions created before end of P
> + all adjustments known at preparation − committed payouts, 0)`.
>
> **Debt** = `max(paid payouts − lifetime net, 0)`.

Why this is correct and convenient:

- **The lock is obeyed by itself.** We pay only at `end of P + 7d`, and the sum includes only
  commissions with `created_at < end of P` — i.e. everything that has already sat for ≥ 7 days.
  Nothing "younger than the lock" makes it into a payout.
- **Rollover is automatic.** If a partner was not paid in the last window (no wallet bound),
  `SUM(paid)` did not grow, and in the next window the `end` is already later — so the sum will
  include **both periods at once** ("next time it will arrive covering 2 periods").
- **No separate accruals table.** Earnings = the commission rows themselves (`commission_entries`
  for v1 events and `commission_entries_v2` for release-v2, one row per
  usage event, with `created_at`); paid = `payouts.paid`. Period state is not stored —
  it is derived from time.
- **Refunds never rewrite gross history.** Each exact negative adjustment names the funding slice
  of the original payment. If commission was not paid, payable drops immediately. If it was already
  paid, the difference becomes explicit partner debt; future earnings repay it first. The system
  never debits the partner's external wallet automatically.

### Payout condition

**Any amount above zero** is paid out — there is no minimum threshold. A partner makes it into the
"to be paid" list (`eligible`) if:

1. `payable > 0` (there is outstanding earnings), and
2. a valid **BSC wallet** is bound (USDT BEP-20, address `0x…40 hex`).

Otherwise (`reason` = `no_wallet` / `zero`) the amount is **held and rolled** into the next window.
`SALES_MIN_PAYOUT_USD` is the single integer-USD threshold used by both the due list and the real
batch sender. The default/current value is `0`; a positive balance is therefore eligible. At a
nonzero setting the boundary is inclusive (`payable >= minimum`) and smaller balances roll over.

## 4. What the partner sees (`GET /v1/partner/periods`)

- **This period** — how much has accrued in the current (open) period.
- **Locked** — earnings of the period that just ended, on the 7-day lock + unfreeze date.
- **Next payout** — the date of the nearest window and an estimate of the amount that will go out.
- **Payable / debt / paid to date** — positive payable is clamped at zero, but debt is never hidden.
- **Period history** — for each half-month period: gross, refund adjustments, net, phase, payout date.
- Plus a "How payouts work" explanation card and wallet binding.

There is no manual "request withdrawal" — payouts follow the schedule.

## 5. What the admin sees (`GET /v1/admin/payout-list`)

The auto-generated list for the current/last period's window:

- **Ready to pay** — the amount and number of partners ready for payout (wallet + profit ≥ minimum).
- **Held (rolls over)** — the amount and the partners who do not yet qualify (no wallet/below the
  minimum).
- **Total payable** — the positive net balance still available across all partners.
- **Partner debt** — already-paid commission reversed by customer refunds/disputes.
- Table: partner, `payable`, `debt`, wallet (masked), status (`Ready` / `No wallet` / `Below minimum`).

## 6. Config

| Env | Default | Meaning |
|---|---|---|
| `SALES_MIN_PAYOUT_USD` | `0` | One integer-USD threshold for due-list and execution. `0` = pay any positive amount; a nonzero boundary is inclusive. |

The lock (7d) and window (3d) are currently constants in `periods.ts`; they can easily be moved
into config if needed.

## 7. Payout execution

The due list remains a read model. `PayoutService.prepare()` turns its eligible rows into one
transactionally revalidated batch: partner rows are locked in canonical order, current wallet,
status and exact unpaid balance are recomputed at the same period end, and every requested payout
becomes committed. Preparation first verifies BSC mainnet, deployed canonical USDT with 18 decimals,
and pins the current hot-wallet address.

The batch schema also has a nullable `earned_before` checkpoint for that exact period end. This is
the boundary reused by the final pre-transfer signed-balance proof. Existing batches remain readable
with `NULL`, but must be canceled and prepared again before sending.

Before both prepare and send, Sales actively drains usage, funding-lot and payment-reversal feeds to
fresh source heads. It then requires zero incomplete usage allocation, zero missing commission slice
and zero partially reflected reversal. Cursor/source-head evidence is returned to the admin report.
The sync mutex stays held while Sales takes the shared accounting advisory lock and probes all three
Commerce heads again. The final local proof and commitment/signing stay under both fences, so a
refund committed before the probe blocks the payout; one committed after it is ordered after the
payout and becomes explicit debt. A legacy batch without `earned_before` must be canceled and
prepared again. Legacy manual payout rows are reject-only; positive settlement uses this fenced
on-chain flow.

`send()` and per-row retry share one PostgreSQL advisory lock across all API processes. Under that
lock the service re-reads batch state, checks that the configured hot wallet still matches the pin,
simulates, signs offline, stores hash + nonce + raw transaction before broadcast, and waits for the
authoritative receipt. A timeout/network ambiguity stops the queue and leaves the exact transaction
for the poller to reconcile; it is never re-signed with a fresh nonce. A `nonce too low` error is not
treated as delivery: only unanimous read-RPC evidence that the hash is absent and the confirmed nonce
advanced can make it retryable. Only a confirmed receipt marks `payouts.status='paid'`. A definitive
revert becomes retryable; release back to partner balance is allowed only after such a failure and
never while a transaction is `broadcast`.

## 8. Time zone

All boundaries are **UTC**. This makes the calculations deterministic and identical on the server
and in tests. If the business needs a Moscow calendar (1–15 in MSK), it is a point change in
`periods.ts` (constructing dates in the desired zone) + regeneration of the tests; it does not
affect the payout invariant.
