import { describe, expect, it } from "vitest";
import {
  NANO_PER_USD,
  buildProductMetrics,
  calculateScenario,
  decimalUsdToNano,
  nanoToEditableUsd,
} from "./calculation";

describe("sales calibration calculator", () => {
  it("parses editable USD as integer nanoUSD without float money", () => {
    expect(decimalUsdToNano("20")).toBe(20_000_000_000n);
    expect(decimalUsdToNano("36.515628714")).toBe(36_515_628_714n);
    expect(nanoToEditableUsd(36_515_628_714n)).toBe("36.515628714");
    expect(decimalUsdToNano("1.0000000001")).toBeNull();
    expect(decimalUsdToNano("-1")).toBeNull();
  });

  it("groups real evidence by paid plan and excludes Claude priors", () => {
    const metrics = buildProductMetrics({
      capacity: {
        per_sub: [
          { email: "same…", plan: "pro", calibrated: true, cap5h_usd: 10, cap7d_usd: 100 },
          { email: "same…", plan: "max5", calibrated: false, cap5h_usd: 12.5, cap7d_usd: 375 },
        ],
      },
      codex: {
        homes: [
          {
            plan: "chatgpt_pro",
            windows: [
              { window_minutes: 300, source: "workload_blend", capacity_nano: "20000000000", low_nano: "18000000000", high_nano: "22000000000" },
              { window_minutes: 10_080, source: "workload_blend", capacity_nano: "200000000000", low_nano: "180000000000", high_nano: "220000000000" },
            ],
          },
        ],
      },
      gemini: {
        profiles: [
          {
            plan: "google_ai_pro",
            windows: [
              { window_minutes: 300, source: "workload_blend", cap_usd: 36.5 },
              { window_minutes: 10_080, source: "workload_blend", cap_usd: 219 },
            ],
          },
        ],
      },
    });

    const claudePro = metrics.find((metric) => metric.product.id === "claude-pro");
    const claudeMax5 = metrics.find((metric) => metric.product.id === "claude-max5");
    const chatGptPro = metrics.find((metric) => metric.product.id === "chatgpt-pro");
    const geminiPro = metrics.find((metric) => metric.product.id === "google-ai-pro");

    expect(claudePro?.fiveHour.capacityNano).toBe(10n * NANO_PER_USD);
    expect(claudePro?.month.capacityNano).toBe((100n * NANO_PER_USD * 30n) / 7n);
    expect(claudeMax5?.profiles).toBe(1);
    expect(claudeMax5?.month.capacityNano).toBeNull();
    expect(chatGptPro?.month.capacityNano).toBe((200n * NANO_PER_USD * 30n) / 7n);
    expect(chatGptPro?.month.lowNano).toBe((180n * NANO_PER_USD * 30n) / 7n);
    expect(geminiPro?.month.capacityNano).toBe((219n * NANO_PER_USD * 30n) / 7n);
  });

  it("calculates discount, underuse, missed revenue and margin in nanoUSD", () => {
    const result = calculateScenario({
      monthlyCapacityNano: 1_000n * NANO_PER_USD,
      quantity: 2,
      utilizationBp: 5_000,
      discountBp: 2_000,
      subscriptionCostNano: 20n * NANO_PER_USD,
    });

    expect(result.fullCapacityNano).toBe(2_000n * NANO_PER_USD);
    expect(result.usedCapacityNano).toBe(1_000n * NANO_PER_USD);
    expect(result.offerNano).toBe(800n * NANO_PER_USD);
    expect(result.customerApiSavingsNano).toBe(200n * NANO_PER_USD);
    expect(result.unusedCapacityNano).toBe(1_000n * NANO_PER_USD);
    expect(result.missedRevenueNano).toBe(800n * NANO_PER_USD);
    expect(result.subscriptionSpendNano).toBe(40n * NANO_PER_USD);
    expect(result.idleSubscriptionSpendNano).toBe(20n * NANO_PER_USD);
    expect(result.grossMarginNano).toBe(760n * NANO_PER_USD);
  });
});
