import { describe, expect, it } from "vitest";
import { chartRatio, usageChartGeometry } from "./usage-chart-data";

describe("usage trend geometry", () => {
  it("preserves zero days and uses one scale for official and charged amounts", () => {
    const geometry = usageChartGeometry([{ official: 0n, charged: 0n }, { official: 4_000_000_000n, charged: 2_000_000_000n }, { official: 0n, charged: 0n }]);
    expect(geometry.officialLine).toBe("M0.00,240.00 L500.00,20.00 L1000.00,240.00");
    expect(geometry.chargedLine).toBe("M0.00,240.00 L500.00,130.00 L1000.00,240.00");
    expect(geometry.ticks.map(tick => tick.value)).toEqual([4_000_000_000n, 3_000_000_000n, 2_000_000_000n, 1_000_000_000n, 0n]);
  });
  it("includes charges above the official value and keeps huge amounts bounded", () => {
    const huge = 10n ** 40n;
    const geometry = usageChartGeometry([{ official: huge / 2n, charged: huge }]);
    expect(geometry.maximum).toBe(huge);
    expect(geometry.x(0)).toBe(500);
    expect(geometry.y(huge)).toBeGreaterThanOrEqual(20);
    expect(geometry.y(huge)).toBeLessThan(240);
    expect(chartRatio(huge / 2n, huge)).toBe(.5);
  });
  it("handles empty data and nanodollar amounts without invalid or flat paths", () => {
    expect(usageChartGeometry([]).chargedArea).toBe("");
    const tiny = usageChartGeometry([{ official: 1n, charged: 1n }]);
    expect(tiny.y(1n)).toBe(185);
    expect(tiny.chargedLine).toBe("M500.00,185.00");
    expect(chartRatio(0n, 0n)).toBe(0);
    expect(chartRatio(3n, 2n)).toBe(1);
  });
});
