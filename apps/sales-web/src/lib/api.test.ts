import { describe, expect, it } from "vitest";
import {
  formatUsd,
  formatUsdCompact,
  isCanonicalNanoUsd,
  isCanonicalSignedNanoUsd,
  isPositiveNanoUsd,
  parseCanonicalNanoUsd,
  parseCanonicalSignedNanoUsd,
  sumCanonicalNanoUsd,
} from "./api";

describe("nanoUSD API contract", () => {
  it("accepts only canonical unsigned decimal strings", () => {
    expect(isCanonicalNanoUsd("0")).toBe(true);
    expect(isCanonicalNanoUsd("1000000001")).toBe(true);
    for (const value of [null, undefined, 0, "", " 1", "1 ", "01", "+1", "-1", "1.0", "1e9"]) {
      expect(isCanonicalNanoUsd(value)).toBe(false);
    }
  });

  it("distinguishes positive amounts and never coerces malformed money", () => {
    expect(isPositiveNanoUsd("0")).toBe(false);
    expect(isPositiveNanoUsd("1")).toBe(true);
    expect(parseCanonicalNanoUsd("0001")).toBeNull();
    expect(sumCanonicalNanoUsd(["1", "2", "3"])).toBe("6");
    expect(sumCanonicalNanoUsd(["1", "bad"])).toBeNull();
  });

  it("accepts canonical negative balances for display without weakening unsigned payout money", () => {
    expect(isCanonicalSignedNanoUsd("-1000000001")).toBe(true);
    expect(parseCanonicalSignedNanoUsd("-1000000001")).toBe(-1000000001n);
    expect(isCanonicalNanoUsd("-1000000001")).toBe(false);
    for (const value of ["-0", "-01", " -1", "-1 ", "--1"]) {
      expect(isCanonicalSignedNanoUsd(value)).toBe(false);
    }
  });

  it("renders valid zero exactly and malformed API money as unavailable", () => {
    expect(formatUsd("0")).toBe("$0.00");
    expect(formatUsd("1234567890123")).toBe("$1,234.56");
    expect(formatUsd("-1234567890123")).toBe("−$1,234.56");
    expect(formatUsd(null)).toBe("—");
    expect(formatUsd("01")).toBe("—");
    expect(formatUsd("garbage")).toBe("—");
    expect(formatUsdCompact("1200000000000")).toBe("$1.2k");
    expect(formatUsdCompact("-1200000000000")).toBe("−$1.2k");
    expect(formatUsdCompact(" 1200000000000")).toBe("—");
  });
});
