import { describe, expect, it } from "vitest";
import { chartRatio, usageChartGeometry } from "./usage-chart-data";
import { buildUtcProviderUsageSeries } from "@/lib/usage-series";

describe("provider usage chart semantics", () => {
  it("stacks original provider amounts and explicit unattributed usage", () => {
    const points = buildUtcProviderUsageSeries(0, 3 * 86400,
      [{ dayTs: 86400, requests: 5, officialNano: "4000000000", chargedNano: "1600000000" }],
      [{ dayTs: 86400, provider: "anthropic", requests: 2, officialNano: "2000000000", chargedNano: "800000000" }, { dayTs: 86400, provider: "openai", requests: 1, officialNano: "1000000000", chargedNano: "400000000" }],
    ).map(point => ({ ...point, official: BigInt(point.officialNano) }));
    const geometry = usageChartGeometry(points, ["anthropic", "openai", null]);
    expect(geometry.layers.map(layer => layer.values)).toEqual([[0n, 2000000000n, 0n], [0n, 1000000000n, 0n], [0n, 1000000000n, 0n]]);
    expect(geometry.layers[1]!.lower).toEqual(geometry.layers[0]!.upper);
    expect(geometry.layers[2]!.upper).toEqual(points.map(point => point.official));
    expect(geometry.ticks.map(tick => tick.value)).toEqual([4000000000n, 3000000000n, 2000000000n, 1000000000n, 0n]);
  });
  it("uses official value alone for scale; charges cannot change plotted values", () => {
    const point = { official: 4n, charged: 100000000000n, providers: [{ provider: "other", officialNano: "4" }], unattributedOfficialNano: "0" };
    const geometry = usageChartGeometry([point], ["other"]);
    expect(geometry.maximum).toBe(4n);
    expect(geometry.layers[0]!.values).toEqual([4n]);
    expect(geometry.layers[0]!.line).toBe("M500.00,20.00");
  });
  it("handles empty data, absent providers, huge amounts and nanodollars", () => {
    expect(usageChartGeometry([], []).layers).toEqual([]);
    const huge = 10n ** 40n;
    expect(chartRatio(huge / 2n, huge)).toBe(.5);
    const tiny = usageChartGeometry([{ official: 1n, providers: [], unattributedOfficialNano: "1" }], ["absent", null]);
    expect(tiny.layers[0]!.values).toEqual([0n]);
    expect(tiny.layers[1]!.values).toEqual([1n]);
    expect(tiny.layers[1]!.line).toBe("M500.00,185.00");
    expect(chartRatio(0n, 0n)).toBe(0);
  });
});
