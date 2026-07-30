import { describe, expect, it } from "vitest";
import { FLAT_DISCOUNT_PERCENT, FLAT_PRICE_MULTIPLIER, formatWholeUsd } from "./pricing-tiers";

describe("flat pricing", () => {
  it("keeps a single 50% discount for everyone", () => {
    expect(FLAT_DISCOUNT_PERCENT).toBe(50);
    expect(FLAT_PRICE_MULTIPLIER).toBe(0.5);
  });

  it("formats whole USD amounts", () => {
    expect(formatWholeUsd("2500")).toBe("$2,500");
  });
});
