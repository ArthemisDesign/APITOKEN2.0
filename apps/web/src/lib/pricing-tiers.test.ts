import { describe, expect, it } from "vitest";
import { B2C_PRICING_MILESTONES, formatWholeUsd, pricingMilestoneProgress } from "./pricing-tiers";

describe("B2C pricing milestones", () => {
  it("keeps the client-facing thresholds and discounts in one ordered model", () => {
    expect(B2C_PRICING_MILESTONES.map(({ code, discountPercent, platformSpendUsd }) => ({ code, discountPercent, platformSpendUsd }))).toEqual([
      { code: "starter", discountPercent: 60, platformSpendUsd: "0" },
      { code: "builder", discountPercent: 62.5, platformSpendUsd: "100" },
      { code: "pro", discountPercent: 65, platformSpendUsd: "250" },
      { code: "studio", discountPercent: 67.5, platformSpendUsd: "500" },
      { code: "scale", discountPercent: 70, platformSpendUsd: "1000" },
    ]);
    expect(formatWholeUsd("2500")).toBe("$2,500");
  });

  it("fills the track so the current tier's dot sits at its own position", () => {
    // Dots are one-per-column with the track ends aligned to the first/last dot, so
    // dot k is at k/(segments-1). Fill = (index + within)/(segments-1) * 100 (5 tiers → /4).
    expect(pricingMilestoneProgress("starter", "0")).toBe(0);
    expect(pricingMilestoneProgress("starter", "50000000000")).toBe(12.5);
    expect(pricingMilestoneProgress("builder", "175000000000")).toBe(37.5);
    expect(pricingMilestoneProgress("pro", "375000000000")).toBe(62.5);
    expect(pricingMilestoneProgress("studio", "750000000000")).toBe(87.5);
    expect(pricingMilestoneProgress("scale", "1000000000000")).toBe(100);
  });

  it("clamps spend inside the active segment", () => {
    // Reaching the next threshold fills to that next dot; 0 progress sits on the current dot.
    expect(pricingMilestoneProgress("starter", "100000000000")).toBe(25);
    expect(pricingMilestoneProgress("builder", "0")).toBe(25);
  });
});
