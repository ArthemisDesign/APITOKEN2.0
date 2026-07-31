import { describe, expect, it } from "vitest";
import { FLAT_DISCOUNT_PERCENT, FLAT_PRICE_MULTIPLIER } from "./pricing-tiers";

describe("flat pricing", () => {
  it("keeps a single 50% B2C discount", () => {
    expect(FLAT_DISCOUNT_PERCENT).toBe(50);
    expect(FLAT_PRICE_MULTIPLIER).toBe(0.5);
  });
});
