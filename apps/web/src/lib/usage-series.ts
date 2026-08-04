import type { UsageDailyProviderRow, UsageDailyRow } from "@/lib/api";

const SECONDS_PER_DAY = 86_400;

export interface UsageSeriesPoint {
  dayTs: number;
  requests: number;
  officialNano: string;
  chargedNano: string;
}

export interface UsageProviderSegment {
  provider: string;
  requests: number;
  officialNano: string;
  chargedNano: string;
}

export interface ProviderUsageSeriesPoint extends UsageSeriesPoint {
  providers: UsageProviderSegment[];
  unattributedOfficialNano: string;
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

/**
 * Joins the authoritative daily total with its provider rows without losing old usage that
 * predates provider attribution. Duplicate provider rows are folded defensively; any uncovered
 * part of the daily official total remains explicit instead of being assigned to a guessed provider.
 */
export function buildUtcProviderUsageSeries(
  sinceTs: number,
  untilTs: number,
  daily: UsageDailyRow[],
  dailyProviders: UsageDailyProviderRow[],
): ProviderUsageSeriesPoint[] {
  const providersByDay = new Map<number, Map<string, {
    requests: number;
    officialNano: bigint;
    chargedNano: bigint;
  }>>();

  for (const row of dailyProviders) {
    const day = providersByDay.get(row.dayTs) ?? new Map();
    const current = day.get(row.provider) ?? { requests: 0, officialNano: 0n, chargedNano: 0n };
    current.requests += row.requests;
    current.officialNano += BigInt(row.officialNano);
    current.chargedNano += BigInt(row.chargedNano);
    day.set(row.provider, current);
    providersByDay.set(row.dayTs, day);
  }

  return buildUtcUsageSeries(sinceTs, untilTs, daily).map((point) => {
    const providers = [...(providersByDay.get(point.dayTs)?.entries() ?? [])]
      .map(([provider, row]) => ({
        provider,
        requests: row.requests,
        officialNano: row.officialNano.toString(),
        chargedNano: row.chargedNano.toString(),
      }))
      .sort((left, right) => left.provider.localeCompare(right.provider));
    const attributedOfficialNano = providers.reduce(
      (total, provider) => total + BigInt(provider.officialNano),
      0n,
    );
    const officialNano = BigInt(point.officialNano);

    return {
      ...point,
      providers,
      unattributedOfficialNano: (officialNano > attributedOfficialNano
        ? officialNano - attributedOfficialNano
        : 0n).toString(),
    };
  });
}

export function usageWindowDays(sinceTs: number, untilTs: number): number {
  if (!Number.isSafeInteger(sinceTs) || !Number.isSafeInteger(untilTs) || untilTs <= sinceTs) {
    return 1;
  }
  return Math.max(1, Math.ceil((untilTs - sinceTs) / SECONDS_PER_DAY));
}
