import { describe, expect, it } from "vitest";
import { parsePercentBps } from "./helpers";

describe("parsePercentBps", () => {
  it("parses exact decimal percentages without floating-point drift", () => {
    expect(parsePercentBps("0.29", 10_000)).toBe(29);
    expect(parsePercentBps("19.99", 10_000)).toBe(1_999);
    expect(parsePercentBps("100.00", 10_000)).toBe(10_000);
  });

  it("rejects excess precision, leading zeros and values above the ceiling", () => {
    expect(parsePercentBps("10.001", 10_000)).toBeNull();
    expect(parsePercentBps("01", 10_000)).toBeNull();
    expect(parsePercentBps("20.01", 2_000)).toBeNull();
  });
});
