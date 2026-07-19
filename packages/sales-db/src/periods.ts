// Semi-monthly payout periods for the partner program.
//
// Two periods per calendar month (UTC):
//   P1 = [day 1 00:00, day 16 00:00)   → days 1–15
//   P2 = [day 16 00:00, next month 1st) → days 16–end
//
// Lifecycle of every period:
//   accruing  — commissions accumulate (now < end)
//   locked    — 7-day hold after the period ends (end ≤ now < end+7d)
//   payable   — 3-day payout window (end+7d ≤ now < end+10d): the due list is generated
//   closed    — window passed (now ≥ end+10d); anything unpaid rolls into a later window
//
// All math is pure and UTC-based so it is deterministic and unit-testable. Money is not handled
// here — this module only answers "which period is a timestamp in and what phase is it".

export const LOCK_DAYS = 7;
export const WINDOW_DAYS = 3;
const DAY_MS = 86_400_000;

export type PeriodPhase = "accruing" | "locked" | "payable" | "closed";

export interface Period {
  /** Stable key, e.g. "2026-07-P1". */
  key: string;
  year: number;
  month: number; // 1–12
  index: 1 | 2;
  start: Date; // inclusive
  end: Date; // exclusive (= next period start)
}

function utc(year: number, month: number, day: number): Date {
  // month is 1–12; Date.UTC takes 0–11.
  return new Date(Date.UTC(year, month - 1, day, 0, 0, 0, 0));
}

function makePeriod(year: number, month: number, index: 1 | 2): Period {
  const start = index === 1 ? utc(year, month, 1) : utc(year, month, 16);
  const end = index === 1 ? utc(year, month, 16) : utc(year, month + 1, 1); // Date.UTC rolls month over
  const key = `${year}-${String(month).padStart(2, "0")}-P${index}`;
  return { key, year, month, index, start, end };
}

/** The period that contains `date`. */
export function periodFor(date: Date): Period {
  const year = date.getUTCFullYear();
  const month = date.getUTCMonth() + 1;
  const day = date.getUTCDate();
  return makePeriod(year, month, day <= 15 ? 1 : 2);
}

export function nextPeriod(period: Period): Period {
  if (period.index === 1) return makePeriod(period.year, period.month, 2);
  // P2 → P1 of next month; normalize December → January.
  const nextMonthStart = period.end; // already the 1st of next month
  return makePeriod(nextMonthStart.getUTCFullYear(), nextMonthStart.getUTCMonth() + 1, 1);
}

export function previousPeriod(period: Period): Period {
  if (period.index === 2) return makePeriod(period.year, period.month, 1);
  // P1 → P2 of previous month.
  const prevMonthLast = new Date(period.start.getTime() - DAY_MS);
  return makePeriod(prevMonthLast.getUTCFullYear(), prevMonthLast.getUTCMonth() + 1, 2);
}

export function lockUntil(period: Period): Date {
  return new Date(period.end.getTime() + LOCK_DAYS * DAY_MS);
}

/** Payout window opens when the lock ends. */
export function windowStart(period: Period): Date {
  return lockUntil(period);
}

export function windowEnd(period: Period): Date {
  return new Date(lockUntil(period).getTime() + WINDOW_DAYS * DAY_MS);
}

export function phaseOf(period: Period, now: Date): PeriodPhase {
  if (now < period.end) return "accruing";
  if (now < lockUntil(period)) return "locked";
  if (now < windowEnd(period)) return "payable";
  return "closed";
}

/**
 * The period whose payout window is currently open, if any. Used to decide when the admin
 * due-list should be generated. Returns null outside every window.
 */
export function periodInPayoutWindow(now: Date): Period | null {
  // A window belongs to a period that ended 7–10 days ago. Check the two most recently ended.
  let candidate = previousPeriod(periodFor(now));
  for (let i = 0; i < 3; i += 1) {
    if (phaseOf(candidate, now) === "payable") return candidate;
    candidate = previousPeriod(candidate);
  }
  return null;
}

/** The most recently ENDED period at `now` (its accrual is final; it is locked or later). */
export function lastEndedPeriod(now: Date): Period {
  const current = periodFor(now);
  return previousPeriod(current);
}
