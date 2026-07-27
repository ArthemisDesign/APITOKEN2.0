import { describe, expect, it } from "vitest";
import { buildUtcUsageSeries, usageWindowDays } from "./usage-series";

const DAY = 86_400;

describe("usage series", () => {
  it("fills UTC calendar buckets across one exact half-open window", () => {
    const sinceTs = 10 * DAY + 3_600;
    const untilTs = 12 * DAY + 7_200;

    expect(buildUtcUsageSeries(sinceTs, untilTs, [{
      dayTs: 11 * DAY,
      requests: 2,
      officialNano: "700",
      chargedNano: "280",
    }])).toEqual([
      { dayTs: 10 * DAY, requests: 0, officialNano: "0", chargedNano: "0" },
      { dayTs: 11 * DAY, requests: 2, officialNano: "700", chargedNano: "280" },
      { dayTs: 12 * DAY, requests: 0, officialNano: "0", chargedNano: "0" },
    ]);
  });

  it("uses the exact elapsed window for daily averages", () => {
    expect(usageWindowDays(100, 100 + 30 * DAY)).toBe(30);
    expect(usageWindowDays(100, 100)).toBe(1);
  });
});
