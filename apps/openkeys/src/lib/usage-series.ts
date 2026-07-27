export interface UsageDailyRow {
  dayTs: number;
  requests: number;
  officialNano: string;
  chargedNano: string;
}

const SECONDS_PER_DAY = 86_400;

export interface UsageSeriesPoint {
  dayTs: number;
  requests: number;
  officialNano: string;
  chargedNano: string;
}

export function buildUtcUsageSeries(
  sinceTs: number,
  untilTs: number,
  daily: UsageDailyRow[],
): UsageSeriesPoint[] {
  if (!Number.isSafeInteger(sinceTs) || !Number.isSafeInteger(untilTs) || untilTs <= sinceTs) {
    return [];
  }

  const firstDayTs = Math.floor(sinceTs / SECONDS_PER_DAY) * SECONDS_PER_DAY;
  const lastDayTs = Math.floor((untilTs - 1) / SECONDS_PER_DAY) * SECONDS_PER_DAY;
  const byDay = new Map(daily.map((row) => [row.dayTs, row]));
  const series: UsageSeriesPoint[] = [];

  for (let dayTs = firstDayTs; dayTs <= lastDayTs; dayTs += SECONDS_PER_DAY) {
    const row = byDay.get(dayTs);
    series.push(row ?? { dayTs, requests: 0, officialNano: "0", chargedNano: "0" });
  }

  return series;
}

export function usageWindowDays(sinceTs: number, untilTs: number): number {
  if (!Number.isSafeInteger(sinceTs) || !Number.isSafeInteger(untilTs) || untilTs <= sinceTs) {
    return 1;
  }
  return Math.max(1, Math.ceil((untilTs - sinceTs) / SECONDS_PER_DAY));
}
