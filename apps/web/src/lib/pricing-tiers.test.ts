import { describe, expect, it } from "vitest";
import { B2C_DISCOUNT_PERCENT, B2C_PAYMENT_RATIO, B2C_VALUE_MULTIPLIER, officialUsageForTopup } from "./pricing-tiers";

describe("flat B2C pricing", () => {
  it("pins one 50% discount for every request and any top-up amount", () => {
    expect(B2C_DISCOUNT_PERCENT).toBe(50);
    expect(B2C_PAYMENT_RATIO).toBe(0.5);
    expect(B2C_VALUE_MULTIPLIER).toBe(2);
  });

  it("doubles every top-up into official API usage, with no tiers or thresholds", () => {
    expect(officialUsageForTopup(50)).toBe(100);
    expect(officialUsageForTopup(10)).toBe(20);
    expect(officialUsageForTopup(1000)).toBe(2000);
  });
});
