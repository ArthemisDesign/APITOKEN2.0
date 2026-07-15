import { describe, expect, it } from "vitest";
import { B2C_PRICING_MILESTONES, formatWholeUsd, pricingMilestoneProgress } from "./pricing-tiers";

describe("B2C pricing milestones", () => {
  it("keeps the client-facing thresholds and discounts in one ordered model", () => {
    expect(B2C_PRICING_MILESTONES.map(({ code, discountPercent, platformSpendUsd }) => ({ code, discountPercent, platformSpendUsd }))).toEqual([
      { code: "starter", discountPercent: 60, platformSpendUsd: "0" },
      { code: "builder", discountPercent: 65, platformSpendUsd: "100" },
      { code: "pro", discountPercent: 70, platformSpendUsd: "250" },
      { code: "studio", discountPercent: 75, platformSpendUsd: "500" },
      { code: "scale", discountPercent: 80, platformSpendUsd: "1000" },
    ]);
    expect(formatWholeUsd("2500")).toBe("$2,500");
  });

  it("fills each visual segment using its real spend interval", () => {
    expect(pricingMilestoneProgress("starter", "0")).toBe(20);
    expect(pricingMilestoneProgress("starter", "50000000000")).toBe(30);
    expect(pricingMilestoneProgress("builder", "175000000000")).toBe(50);
    expect(pricingMilestoneProgress("pro", "375000000000")).toBe(70);
    expect(pricingMilestoneProgress("studio", "750000000000")).toBe(90);
    expect(pricingMilestoneProgress("scale", "1000000000000")).toBe(100);
  });

  it("clamps spend inside the active segment", () => {
    expect(pricingMilestoneProgress("starter", "100000000000")).toBe(40);
    expect(pricingMilestoneProgress("builder", "0")).toBe(40);
  });
});
