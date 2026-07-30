export const ADMIN_USAGE_FILTERS = ["all", "unused", "used", "exhausted", "unavailable"] as const;
export type AdminUsageFilter = (typeof ADMIN_USAGE_FILTERS)[number];
export type AdminUsageState = Exclude<AdminUsageFilter, "all">;

export function parseAdminUsageFilter(value: string | null): AdminUsageFilter | null {
  return ADMIN_USAGE_FILTERS.includes(value as AdminUsageFilter) ? value as AdminUsageFilter : null;
}

export function classifyAdminUsage(
  spentNano: bigint | null,
  remainingNano: bigint | null,
): AdminUsageState {
  if (spentNano === null || remainingNano === null) return "unavailable";
  if (remainingNano <= 0n) return "exhausted";
  return spentNano <= 0n ? "unused" : "used";
}

/** Целый процент без float-арифметики над деньгами; значение может быть больше 100 при перерасходе. */
export function adminUsagePercent(spentNano: bigint | null, faceValueNano: bigint): number | null {
  if (spentNano === null || faceValueNano <= 0n) return null;
  if (spentNano <= 0n) return 0;
  return Number((spentNano * 100n + faceValueNano - 1n) / faceValueNano);
}

export function matchesAdminUsage(state: AdminUsageState, filter: AdminUsageFilter): boolean {
  return filter === "all" || state === filter;
}
