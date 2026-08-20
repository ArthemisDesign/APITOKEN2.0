import { describe, expect, it } from "vitest";
import {
  batchCount,
  geminiBatchWarnings,
  historyThroughputPercent,
  orderedBatchHistory,
} from "./gemini-batch-summary";

describe("Gemini Batch admin summary helpers", () => {
  it("orders operational warnings by settlement, indeterminate, leader and stale queue", () => {
    expect(geminiBatchWarnings({
      queued_items: 7,
      settlement_pending_items: 2,
      settlement_oldest_age_seconds: 600,
      indeterminate_items: 1,
      leader_held: false,
      oldest_queued_age_seconds: 3_600,
    }).map((warning) => warning.code)).toEqual([
      "settlement_backlog",
      "indeterminate",
      "leader_missing",
      "queue_stale",
    ]);
  });

  it("does not warn about an absent leader when there is no queued work", () => {
    expect(geminiBatchWarnings({ leader_held: false, queued_items: 0 })).toEqual([]);
  });

  it("orders only the closed history windows and scales throughput against the peak", () => {
    const history = orderedBatchHistory([
      { window: "7d", throughput_items_per_hour: 25 },
      { window: "1h", throughput_items_per_hour: 100 },
      { window: "24h", throughput_items_per_hour: 50 },
      { window: "future" as "1h", throughput_items_per_hour: 999 },
    ]);
    expect(history.map((row) => row.window)).toEqual(["1h", "24h", "7d"]);
    expect(historyThroughputPercent(history[1], history)).toBe(50);
    expect(historyThroughputPercent(history[2], history)).toBe(25);
  });

  it("prefers additive fields but retains the old snapshot fallback", () => {
    expect(batchCount(3, 9)).toBe(3);
    expect(batchCount(undefined, 9)).toBe(9);
    expect(batchCount(-1)).toBe(0);
  });
});
