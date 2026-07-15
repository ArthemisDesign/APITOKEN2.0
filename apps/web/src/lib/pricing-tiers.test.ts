import { describe, expect, it } from "vitest";
import { B2C_PRICING_MILESTONES, formatWholeUsd, pricingMilestoneProgress } from "./pricing-tiers";

describe("B2C pricing milestones", () => {
  it("keeps the client-facing thresholds and discounts in one ordered model", () => {
    expect(B2C_PRICING_MILESTONES.map(({ code, discountPercent, platformSpendUsd }) => ({ code, discountPercent, platformSpendUsd }))).toEqual([
      { code: "starter", discountPercent: 60, platformSpendUsd: "0" },
      { code: "builder", discountPercent: 65, platformSpendUsd: "25" },
      { code: "pro", discountPercent: 70, platformSpendUsd: "75" },
      { code: "studio", discountPercent: 75, platformSpendUsd: "200" },
      { code: "scale", discountPercent: 80, platformSpendUsd: "500" },
    ]);
    expect(formatWholeUsd("2500")).toBe("$2,500");
  });

  it("fills each visual segment using its real spend interval", () => {
    expect(pricingMilestoneProgress("starter", "0")).toBe(0);
    expect(pricingMilestoneProgress("starter", "12500000000")).toBe(12.5);
    expect(pricingMilestoneProgress("builder", "50000000000")).toBe(37.5);
    expect(pricingMilestoneProgress("pro", "137500000000")).toBe(62.5);
    expect(pricingMilestoneProgress("studio", "350000000000")).toBe(87.5);
    expect(pricingMilestoneProgress("scale", "500000000000")).toBe(100);
  });

  it("clamps spend inside the active segment", () => {
    expect(pricingMilestoneProgress("starter", "50000000000")).toBe(25);
    expect(pricingMilestoneProgress("builder", "0")).toBe(25);
  });
});
