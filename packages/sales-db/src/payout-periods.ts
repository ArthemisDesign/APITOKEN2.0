import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";
import {
  lastEndedPeriod,
  lockUntil,
  nextPeriod,
  periodFor,
  periodInPayoutWindow,
  phaseOf,
  windowEnd,
  windowStart,
  type Period,
  type PeriodPhase,
} from "./periods.js";

// DB-backed view over the semi-monthly payout periods. Earnings are bucketed from
// commission_entries by created_at (UTC); "paid" is subtracted from payouts. The actual payout
// EXECUTION (on-chain sending / marking) is a separate later system — here everything is a
// read model that the cabinet and the admin due-list render.

export interface PartnerPeriodState {
  now: string;
  current: { key: string; start: string; end: string; accruedNano: string };
  locked: { key: string; endedAt: string; unlocksAt: string; earnedNano: string }[];
  nextPayout: { date: string; estimatedNano: string };
  lifetimeEarnedNano: string;
  lifetimePaidNano: string;
  unpaidNano: string;
}

export interface PeriodHistoryRow {
  key: string;
  index: 1 | 2;
  start: string;
  end: string;
  phase: PeriodPhase;
  payoutDate: string; // window start
  earnedNano: string;
}

export interface DuePayoutRow {
  partnerId: string;
  telegramUsername: string | null;
  displayName: string | null;
  status: PartnerStatus;
  payableNano: string;
  walletAddress: string | null;
  eligible: boolean;
  reason: "ok" | "below_minimum" | "no_wallet" | "zero" | "inactive";
}

function periodSummary(p: Period) {
  return { key: p.key, start: p.start.toISOString(), end: p.end.toISOString() };
}

/** SUM комиссий партнёра за [from, to) по ОБЕИМ schema (v1 + v2; события не пересекаются,
 * двойного счёта нет). Границы могут быть null (открытые). */
async function sumCommissions(
  database: SalesDatabase,
  partnerId: string,
  from: Date | null,
  to: Date | null,
): Promise<bigint> {
  const conditions = ["partner_id = $1"];
  const params: unknown[] = [partnerId];
  if (from) { params.push(from); conditions.push(`created_at >= $${params.length}`); }
  if (to) { params.push(to); conditions.push(`created_at < $${params.length}`); }
  const result = await database.pool.query<{ total: string }>(
    `SELECT COALESCE(SUM(amount_nano), 0)::text AS total FROM (
       SELECT partner_id, amount_nano, created_at FROM commission_entries
       UNION ALL
       SELECT partner_id, amount_nano, created_at FROM commission_entries_v2
     ) all_commissions WHERE ${conditions.join(" AND ")}`,
    params,
  );
  return BigInt(result.rows[0]?.total ?? "0");
}

async function sumPaid(database: SalesDatabase, partnerId: string): Promise<bigint> {
  const result = await database.pool.query<{ total: string }>(
    "SELECT COALESCE(SUM(amount_nano), 0)::text AS total FROM payouts WHERE partner_id = $1 AND status = 'paid'",
    [partnerId],
  );
  return BigInt(result.rows[0]?.total ?? "0");
}

// Уже «зафиксированные» деньги: выплачено ('paid') ЛИБО в процессе ('requested'/'approved').
// Именно это, а не только 'paid', нужно вычитать из причитающегося — иначе пока выплата обрабатывается,
// та же сумма показывается к оплате повторно и уходит в двойную отправку. 'rejected' не в счёт
// (эти деньги всё ещё причитаются).
async function sumCommitted(database: SalesDatabase, partnerId: string): Promise<bigint> {
  const result = await database.pool.query<{ total: string }>(
    "SELECT COALESCE(SUM(amount_nano), 0)::text AS total FROM payouts WHERE partner_id = $1 AND status IN ('requested', 'approved', 'paid')",
    [partnerId],
  );
  return BigInt(result.rows[0]?.total ?? "0");
}

export async function getPartnerPeriodState(
  database: SalesDatabase,
  partnerId: string,
  now: Date,
): Promise<PartnerPeriodState> {
  const current = periodFor(now);
  const lastEnded = lastEndedPeriod(now);
  const openWindow = periodInPayoutWindow(now);

  const [accrued, lifetimeEarned, paid, committed, lockedEarned] = await Promise.all([
    sumCommissions(database, partnerId, current.start, now),
    sumCommissions(database, partnerId, null, null),
    sumPaid(database, partnerId),
    sumCommitted(database, partnerId),
    // The just-ended period is "locked" until its payout window opens.
    sumCommissions(database, partnerId, lastEnded.start, lastEnded.end),
  ]);

  const locked: PartnerPeriodState["locked"] = [];
  if (phaseOf(lastEnded, now) === "locked" && lockedEarned > 0n) {
    locked.push({
      key: lastEnded.key,
      endedAt: lastEnded.end.toISOString(),
      unlocksAt: windowStart(lastEnded).toISOString(),
      earnedNano: lockedEarned.toString(),
    });
  }

  // «Не выплачено» = заработано − уже зафиксированное (выплачено + в очереди). Вычитаем committed,
  // а не только paid, чтобы сумма в процессе выплаты не считалась причитающейся повторно.
  const unpaid = lifetimeEarned - committed;
  // Дата и сумма следующей выплаты — корректно во время ОТКРЫТОГО окна (иначе кабинет показывал бы
  // выплату «через 2 недели», хотя деньги причитаются к оплате прямо сейчас).
  let nextDate: Date;
  let payablePeriodEnd: Date;
  if (openWindow) {
    nextDate = windowStart(openWindow);   // окно выплат открыто — причитается сейчас
    payablePeriodEnd = openWindow.end;
  } else if (now < windowStart(lastEnded)) {
    nextDate = windowStart(lastEnded);    // прошлый период ещё в 7-дневном локе
    payablePeriodEnd = lastEnded.end;
  } else {
    nextDate = windowStart(current);      // между окнами — ближайшее окно текущего периода
    payablePeriodEnd = current.end;
  }
  const confirmedByThen = await sumCommissions(database, partnerId, null, payablePeriodEnd);
  const estimated = confirmedByThen - committed > 0n ? confirmedByThen - committed : 0n;

  return {
    now: now.toISOString(),
    current: { ...periodSummary(current), accruedNano: accrued.toString() },
    locked,
    nextPayout: { date: nextDate.toISOString(), estimatedNano: estimated.toString() },
    lifetimeEarnedNano: lifetimeEarned.toString(),
    lifetimePaidNano: paid.toString(),
    unpaidNano: (unpaid > 0n ? unpaid : 0n).toString(),
  };
}

/** Per-period earnings history for the cabinet, newest first. */
export async function getPartnerPeriodHistory(
  database: SalesDatabase,
  partnerId: string,
  now: Date,
  limit = 12,
): Promise<PeriodHistoryRow[]> {
  const result = await database.pool.query<{ ym: string; half: string; earned: string }>(
    `
    SELECT to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM') AS ym,
           (CASE WHEN extract(day FROM created_at AT TIME ZONE 'UTC') <= 15 THEN 1 ELSE 2 END)::text AS half,
           SUM(amount_nano)::text AS earned
    FROM (
      SELECT partner_id, amount_nano, created_at FROM commission_entries
      UNION ALL
      SELECT partner_id, amount_nano, created_at FROM commission_entries_v2
    ) all_commissions
    WHERE partner_id = $1
    GROUP BY ym, half
    ORDER BY ym DESC, half DESC
    LIMIT $2
    `,
    [partnerId, limit],
  );
  return result.rows.map((row) => {
    const [year, month] = row.ym.split("-").map((n) => Number(n));
    const anchorDay = row.half === "1" ? 5 : 20;
    const period = periodFor(new Date(Date.UTC(year!, month! - 1, anchorDay)));
    return {
      key: period.key,
      index: period.index,
      start: period.start.toISOString(),
      end: period.end.toISOString(),
      phase: phaseOf(period, now),
      payoutDate: windowStart(period).toISOString(),
      earnedNano: row.earned,
    };
  });
}

/**
 * Auto-generated due list for the payout window. Target period = the one whose window is open now;
 * if none is open, the last ended period is used so the admin can preview the upcoming batch.
 * Payable per partner = confirmed commissions before the period end − already paid (rollover is
 * automatic: unpaid prior periods are included because the cut-off only excludes what is not yet
 * locked). Eligibility requires a bound BSC wallet and payable ≥ minPayout.
 */
export async function getDuePayoutList(
  database: SalesDatabase,
  now: Date,
  minPayoutNano: bigint,
): Promise<{ period: { key: string; start: string; end: string; payoutWindowStart: string; payoutWindowEnd: string; phase: PeriodPhase }; items: DuePayoutRow[] }> {
  const openWindow = periodInPayoutWindow(now);
  const target = openWindow ?? lastEndedPeriod(now);
  // Платить можно только когда окно выплат реально открыто (после 7-дневного лока). Если открытого
  // окна нет, это превью последнего периода — суммы показываем, но никто не «eligible» (ещё в локе).
  const windowOpen = openWindow !== null;

  // ВНИМАНИЕ: вычитаем зафиксированные выплаты (paid + requested + approved), а не только paid —
  // иначе партнёр с выплатой «в процессе» снова попадёт в список к оплате (двойная отправка).
  // Неактивных партнёров НЕ прячем: если у них остался причитающийся баланс (заработан, пока были
  // активны), он должен быть виден админу — но платить их нельзя (eligible=false, reason=inactive).
  const result = await database.pool.query<{
    partner_id: string; telegram_username: string | null; display_name: string | null;
    status: PartnerStatus; payable: string; payout_method: string | null; payout_details: unknown;
  }>(
    `
    SELECT p.id AS partner_id, p.telegram_username, p.display_name, p.status, p.payout_method, p.payout_details,
      (
        COALESCE((SELECT SUM(ce.amount_nano) FROM (
                    SELECT partner_id, amount_nano, created_at FROM commission_entries
                    UNION ALL
                    SELECT partner_id, amount_nano, created_at FROM commission_entries_v2
                  ) ce
                  WHERE ce.partner_id = p.id AND ce.created_at < $1), 0)
        - COALESCE((SELECT SUM(po.amount_nano) FROM payouts po
                    WHERE po.partner_id = p.id AND po.status IN ('requested', 'approved', 'paid')), 0)
      )::text AS payable
    FROM partners p
    `,
    [target.end],
  );

  const items: DuePayoutRow[] = result.rows
    .map((row) => {
      const payable = BigInt(row.payable);
      const wallet = walletAddressOf(row.payout_method, row.payout_details);
      let reason: DuePayoutRow["reason"] = "ok";
      if (payable <= 0n) reason = "zero";
      else if (row.status !== "active") reason = "inactive";
      else if (!wallet) reason = "no_wallet";
      else if (payable < minPayoutNano) reason = "below_minimum";
      return {
        partnerId: row.partner_id,
        telegramUsername: row.telegram_username,
        displayName: row.display_name,
        status: row.status,
        payableNano: (payable > 0n ? payable : 0n).toString(),
        walletAddress: wallet,
        eligible: reason === "ok" && windowOpen,
        reason,
      };
    })
    .filter((row) => BigInt(row.payableNano) > 0n)
    .sort((a, b) => (BigInt(b.payableNano) > BigInt(a.payableNano) ? 1 : -1));

  return {
    period: {
      key: target.key,
      start: target.start.toISOString(),
      end: target.end.toISOString(),
      payoutWindowStart: windowStart(target).toISOString(),
      payoutWindowEnd: windowEnd(target).toISOString(),
      phase: phaseOf(target, now),
    },
    items,
  };
}

function walletAddressOf(method: string | null, details: unknown): string | null {
  if (method !== "usdt-bep20") return null;
  if (details === null || typeof details !== "object" || !("address" in details)) return null;
  const address = (details as { address?: unknown }).address;
  return typeof address === "string" && /^0x[a-fA-F0-9]{40}$/.test(address) ? address : null;
}

export { nextPeriod, lockUntil };
