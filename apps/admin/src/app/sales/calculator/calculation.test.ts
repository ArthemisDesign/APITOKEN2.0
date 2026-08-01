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

  it("scales Claude Pro into Max 5× and Max 20× while excluding cold priors", () => {
    const metrics = buildProductMetrics({
      capacity: {
        per_sub: [
          { email: "same…", plan: "pro", calibrated: true, cap5h_usd: 10, cap7d_usd: 100 },
          { email: "same…", plan: "max5", calibrated: false, cap5h_usd: 12.5, cap7d_usd: 375 },
        ],
      },
      codex: null,
      gemini: null,
    });

    const claudePro = metrics.find((metric) => metric.product.id === "claude-pro");
    const claudeMax5 = metrics.find((metric) => metric.product.id === "claude-max5");
    const claudeMax20 = metrics.find((metric) => metric.product.id === "claude-max20");

    expect(claudePro?.fiveHour.capacityNano).toBe(10n * NANO_PER_USD);
    expect(claudePro?.month.capacityNano).toBe((100n * NANO_PER_USD * 30n) / 7n);
    expect(claudeMax5?.profiles).toBe(1);
    expect(claudeMax5?.measuredProfiles).toBe(0);
    expect(claudeMax5?.fiveHour.capacityNano).toBe(50n * NANO_PER_USD);
    expect(claudeMax5?.sevenDay.capacityNano).toBe(500n * NANO_PER_USD);
    expect(claudeMax5?.month.evidence).toBe("estimated");
    expect(claudeMax5?.month.estimate?.sources[0].ratioLabel).toBe("×5");
    expect(claudeMax20?.fiveHour.capacityNano).toBe(200n * NANO_PER_USD);
    expect(claudeMax20?.sevenDay.capacityNano).toBe(2_000n * NANO_PER_USD);
  });

  it("lets a tariff's own calibration replace its estimate", () => {
    const metrics = buildProductMetrics({
      capacity: {
        per_sub: [
          { plan: "pro", calibrated: true, cap5h_nano: "10000000000", cap7d_nano: "100000000000" },
          { plan: "max5", calibrated: true, cap5h_nano: "55000000000", cap7d_nano: "550000000000" },
        ],
      },
      codex: null,
      gemini: null,
    });

    const claudeMax5 = metrics.find((metric) => metric.product.id === "claude-max5");
    expect(claudeMax5?.fiveHour.capacityNano).toBe(55n * NANO_PER_USD);
    expect(claudeMax5?.fiveHour.evidence).toBe("measured");
    expect(claudeMax5?.fiveHour.estimate).toBeNull();
    expect(claudeMax5?.month.evidence).toBe("measured");
  });

  it("derives Plus, Business and Pro 5× from a live ChatGPT Pro 20×", () => {
    const metrics = buildProductMetrics({
      capacity: null,
      codex: {
        homes: [{
          plan: "chatgpt_pro",
          windows: [
            { window_minutes: 300, source: "workload_blend", capacity_nano: "20000000000" },
            { window_minutes: 10_080, source: "workload_blend", capacity_nano: "200000000000" },
          ],
        }],
      },
      gemini: null,
    });

    const plus = metrics.find((metric) => metric.product.id === "chatgpt-plus");
    const pro5 = metrics.find((metric) => metric.product.id === "chatgpt-pro-5x");
    const pro20 = metrics.find((metric) => metric.product.id === "chatgpt-pro-20x");
    const business = metrics.find((metric) => metric.product.id === "chatgpt-business");
    expect(pro20?.fiveHour.evidence).toBe("measured");
    expect(plus?.fiveHour.capacityNano).toBe(1n * NANO_PER_USD);
    expect(plus?.fiveHour.estimate?.sources[0].ratioLabel).toBe("÷20");
    expect(pro5?.fiveHour.capacityNano).toBe(5n * NANO_PER_USD);
    expect(pro5?.fiveHour.estimate?.sources[0].ratioLabel).toBe("÷4");
    expect(business?.sevenDay.capacityNano).toBe(10n * NANO_PER_USD);
  });

  it("uses the published 1:20 Google AI Pro to Ultra ratio", () => {
    const metrics = buildProductMetrics({
      capacity: null,
      codex: null,
      gemini: {
        profiles: [{
          plan: "google_ai_pro",
          windows: [
            { window_minutes: 300, source: "workload_blend", capacity_nano: "36000000000" },
            { window_minutes: 10_080, source: "workload_blend", capacity_nano: "219000000000" },
          ],
        }],
      },
    });

    const ultra = metrics.find((metric) => metric.product.id === "google-ai-ultra");
    expect(metrics.filter((metric) => metric.product.provider === "gemini").map((metric) => metric.product.id)).toEqual([
      "google-ai-pro",
      "google-ai-ultra",
    ]);
    expect(ultra?.fiveHour.capacityNano).toBe(720n * NANO_PER_USD);
    expect(ultra?.sevenDay.capacityNano).toBe(4_380n * NANO_PER_USD);
    expect(ultra?.fiveHour.estimate?.sources[0].ratioLabel).toBe("×20");
  });

  it("keeps plans unknown when their provider has no measured anchor", () => {
    const metrics = buildProductMetrics({
      capacity: { per_sub: [{ plan: "pro", calibrated: false, cap5h_usd: 10, cap7d_usd: 100 }] },
      codex: { homes: [] },
      gemini: { profiles: [] },
    });

    for (const metric of metrics) {
      expect(metric.fiveHour.evidence).toBe("unknown");
      expect(metric.sevenDay.evidence).toBe("unknown");
      expect(metric.month.capacityNano).toBeNull();
    }
  });

  it("averages normalized direct anchors without recursively reusing estimates", () => {
    const metrics = buildProductMetrics({
      capacity: {
        per_sub: [
          { plan: "pro", calibrated: true, cap5h_nano: "10000000000", cap7d_nano: "100000000000" },
          { plan: "max20", calibrated: true, cap5h_nano: "240000000000", cap7d_nano: "2400000000000" },
        ],
      },
      codex: null,
      gemini: null,
    });

    const max5 = metrics.find((metric) => metric.product.id === "claude-max5");
    expect(max5?.fiveHour.capacityNano).toBe(55n * NANO_PER_USD);
    expect(max5?.sevenDay.capacityNano).toBe(550n * NANO_PER_USD);
    expect(max5?.fiveHour.estimate?.sources.map((source) => source.ratioLabel)).toEqual(["×5", "÷4"]);
    expect(max5?.fiveHour.estimate?.sources).toHaveLength(2);
  });

  it("keeps an estimated envelope only when every direct anchor has one", () => {
    const withCompleteEnvelope = buildProductMetrics({
      capacity: null,
      codex: {
        homes: [{
          plan: "chatgpt_pro",
          windows: [
            { window_minutes: 300, source: "workload_blend", capacity_nano: "20000000000", low_nano: "18000000000", high_nano: "22000000000" },
            { window_minutes: 10_080, source: "workload_blend", capacity_nano: "200000000000", low_nano: "180000000000", high_nano: "220000000000" },
          ],
        }],
      },
      gemini: null,
    });
    const plusFromOne = withCompleteEnvelope.find((metric) => metric.product.id === "chatgpt-plus");
    expect(plusFromOne?.fiveHour.lowNano).toBe(900_000_000n);
    expect(plusFromOne?.fiveHour.highNano).toBe(1_100_000_000n);

    const withIncompleteAnchor = buildProductMetrics({
      capacity: null,
      codex: {
        homes: [
          {
            plan: "chatgpt_plus",
            windows: [
              { window_minutes: 300, source: "workload_blend", capacity_nano: "1000000000", low_nano: "900000000", high_nano: "1100000000" },
              { window_minutes: 10_080, source: "workload_blend", capacity_nano: "10000000000", low_nano: "9000000000", high_nano: "11000000000" },
            ],
          },
          {
            plan: "chatgpt_pro",
            windows: [
              { window_minutes: 300, source: "workload_blend", capacity_nano: "20000000000" },
              { window_minutes: 10_080, source: "workload_blend", capacity_nano: "200000000000" },
            ],
          },
        ],
      },
      gemini: null,
    });
    const pro5 = withIncompleteAnchor.find((metric) => metric.product.id === "chatgpt-pro-5x");
    expect(pro5?.fiveHour.capacityNano).toBe(5n * NANO_PER_USD);
    expect(pro5?.fiveHour.lowNano).toBeNull();
    expect(pro5?.fiveHour.highNano).toBeNull();
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
