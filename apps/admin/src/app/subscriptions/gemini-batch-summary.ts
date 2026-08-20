import type { GeminiBatchHistoryWindow, GeminiBatchSummary } from "./types";

export interface GeminiBatchWarning {
  kind: "warn" | "bad";
  code: "settlement_failed" | "settlement_backlog" | "indeterminate" | "leader_missing" | "queue_stale";
  title: string;
  detail: string;
}

export function batchCount(primary: number | undefined, fallback?: number): number {
  const value = primary ?? fallback ?? 0;
  return Number.isFinite(Number(value)) ? Math.max(0, Number(value)) : 0;
}

export function geminiBatchWarnings(batch: GeminiBatchSummary): GeminiBatchWarning[] {
  const queued = batchCount(batch.queued_items, batch.queue_depth);
  const settlement = batchCount(batch.settlement_pending_items, batch.settlement_backlog);
  const indeterminate = batchCount(batch.indeterminate_items);
  const age = batchCount(batch.oldest_queued_age_seconds);
  const warnings: GeminiBatchWarning[] = [];

  // Operational triage order is deliberate: money first, then ambiguous execution, authority,
  // finally latency. Never include an account/job/profile identity in these fleet-only warnings.
  if (batchCount(batch.settlement_failed) > 0) warnings.push({
    kind: "bad",
    code: "settlement_failed",
    title: "Settlement failed",
    detail: `${batchCount(batch.settlement_failed)} item требуют расследования permanent settlement failure.`,
  });
  if (settlement > 0) warnings.push({
    kind: "bad",
    code: "settlement_backlog",
    title: "Settlement backlog",
    detail: `${settlement} item ожидают применения settlement; старейший ${formatDuration(batch.settlement_oldest_age_seconds)}.`,
  });
  if (indeterminate > 0) warnings.push({
    kind: "bad",
    code: "indeterminate",
    title: "Indeterminate execution",
    detail: `${indeterminate} item пересекли или могли пересечь actual-send без точного terminal результата.`,
  });
  if (batch.leader_held === false && queued > 0) warnings.push({
    kind: "warn",
    code: "leader_missing",
    title: "Нет Batch leader",
    detail: `${queued} item в очереди, но dispatch leader не удерживается.`,
  });
  if (queued > 0 && age > 15 * 60) warnings.push({
    kind: "warn",
    code: "queue_stale",
    title: "Очередь не двигается",
    detail: `Старейший queued item ждёт ${formatDuration(age)}.`,
  });
  return warnings;
}

export function orderedBatchHistory(history: GeminiBatchHistoryWindow[] | undefined): GeminiBatchHistoryWindow[] {
  const order = new Map([["1h", 0], ["24h", 1], ["7d", 2]]);
  return [...(history ?? [])]
    .filter((row) => row.window === "1h" || row.window === "24h" || row.window === "7d")
    .sort((a, b) => (order.get(a.window ?? "") ?? 99) - (order.get(b.window ?? "") ?? 99));
}

export function historyThroughputPercent(
  row: GeminiBatchHistoryWindow,
  history: GeminiBatchHistoryWindow[],
): number {
  const current = Math.max(0, Number(row.throughput_items_per_hour) || 0);
  const peak = history.reduce((max, item) => Math.max(max, Number(item.throughput_items_per_hour) || 0), 0);
  return peak > 0 ? Math.min(100, Math.round((current / peak) * 100)) : 0;
}

function formatDuration(value: number | null | undefined): string {
  const seconds = Math.max(0, Number(value) || 0);
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3_600);
  const minutes = Math.floor((seconds % 3_600) / 60);
  return days ? `${days}д ${hours}ч` : hours ? `${hours}ч ${minutes}м` : `${minutes}м`;
}
