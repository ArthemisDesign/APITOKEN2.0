import { describe, expect, it } from "vitest";
import { buildUtcProviderUsageSeries, buildUtcUsageSeries, usageWindowDays } from "./usage-series";

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

  it("stacks provider rows while preserving an explicit unattributed remainder", () => {
    const sinceTs = 10 * DAY;
    const untilTs = 12 * DAY;

    expect(buildUtcProviderUsageSeries(
      sinceTs,
      untilTs,
      [
        { dayTs: 10 * DAY, requests: 5, officialNano: "1000", chargedNano: "400" },
        { dayTs: 11 * DAY, requests: 1, officialNano: "200", chargedNano: "80" },
      ],
      [
        { dayTs: 10 * DAY, provider: "openai", requests: 2, officialNano: "250", chargedNano: "100" },
        { dayTs: 10 * DAY, provider: "anthropic", requests: 2, officialNano: "500", chargedNano: "200" },
        { dayTs: 10 * DAY, provider: "openai", requests: 1, officialNano: "150", chargedNano: "60" },
        { dayTs: 10 * DAY, provider: "future-provider", requests: 0, officialNano: "50", chargedNano: "20" },
      ],
    )).toEqual([
      {
        dayTs: 10 * DAY,
        requests: 5,
        officialNano: "1000",
        chargedNano: "400",
        providers: [
          { provider: "anthropic", requests: 2, officialNano: "500", chargedNano: "200" },
          { provider: "future-provider", requests: 0, officialNano: "50", chargedNano: "20" },
          { provider: "openai", requests: 3, officialNano: "400", chargedNano: "160" },
        ],
        unattributedOfficialNano: "50",
      },
      {
        dayTs: 11 * DAY,
        requests: 1,
        officialNano: "200",
        chargedNano: "80",
        providers: [],
        unattributedOfficialNano: "200",
      },
    ]);
  });
});
