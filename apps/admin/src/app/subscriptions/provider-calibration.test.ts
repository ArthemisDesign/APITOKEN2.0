import { describe, expect, it } from "vitest";
import {
  compactTokenCount,
  compareProviderRates,
  exactTokenCount,
  formatUsdPerMillion,
  formatUsdPerUnit,
  providerInteger,
  tokensForNanoCapacity,
  usedPercentFromNano,
} from "./provider-calibration";

describe("provider capacity arithmetic", () => {
  it("converts nanoUSD capacity to tokens without JavaScript number", () => {
    expect(tokensForNanoCapacity("120000000000", "5000")).toBe(24_000_000n);
    expect(tokensForNanoCapacity("120000000000", "500")).toBe(240_000_000n);
    expect(tokensForNanoCapacity("120000000000", "0")).toBeNull();
    expect(tokensForNanoCapacity("broken", "5000")).toBeNull();
  });

  it("formats compact and exact token counts", () => {
    expect(compactTokenCount(24_000_000n)).toBe("24M");
    expect(compactTokenCount(2_450_000_000n)).toBe("2.5B");
    expect(exactTokenCount(24_000_001n)).toBe("24,000,001");
  });

  it("formats tariff units and sorts exact rates", () => {
    expect(formatUsdPerMillion("5000")).toBe("$5.00");
    expect(formatUsdPerMillion("25")).toBe("$0.025");
    expect(formatUsdPerUnit("14000000")).toBe("$0.014");
    expect(compareProviderRates("60000", "18000")).toBe(1);
    expect(providerInteger("9007199254740993000")).toBe(9_007_199_254_740_993_000n);
  });

  it("derives a stable used percentage from integer money", () => {
    expect(usedPercentFromNano("300000000000", "195000000000")).toEqual({ value: 35, label: "35%" });
    expect(usedPercentFromNano(null, null)).toEqual({ value: null, label: "—" });
  });
});
