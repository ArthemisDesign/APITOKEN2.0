import { describe, expect, it } from "vitest";
import { dailyProviderTotal, earningsShareTenths, providerLabel } from "./provider-breakdown";

describe("provider earnings breakdown", () => {
  it("names the providers the pool actually serves", () => {
    expect(providerLabel("anthropic", "—")).toBe("Claude");
    expect(providerLabel("openai", "—")).toBe("GPT");
    expect(providerLabel("google", "—")).toBe("Gemini");
    expect(providerLabel("kimi", "—")).toBe("Kimi");
  });

  it("shows an unknown provider by its id instead of hiding the money", () => {
    // A provider added to the pool must show up here immediately, before anyone writes a label.
    expect(providerLabel("brand-new-provider", "—")).toBe("brand-new-provider");
  });

  it("labels pre-migration rows rather than dropping them", () => {
    expect(providerLabel(null, "Before provider tracking")).toBe("Before provider tracking");
  });

  it("computes shares without float math on money", () => {
    expect(earningsShareTenths("250000000", 1_000_000_000n)).toBe(250);
    expect(earningsShareTenths("1000000000", 1_000_000_000n)).toBe(1000);
    expect(earningsShareTenths("0", 1_000_000_000n)).toBe(0);
    // Above 2^53 nano ($9M+), a float path would round; the BigInt path stays exact.
    expect(earningsShareTenths("9007199254740993", 18014398509481986n)).toBe(500);
  });

  it("does not divide by zero before any earnings exist", () => {
    expect(earningsShareTenths("0", 0n)).toBe(0);
  });

  it("keeps historical and known-provider segments in the daily total", () => {
    expect(dailyProviderTotal({
      date: "2026-08-22",
      providers: [
        { providerId: "anthropic", events: 1, spendNano: "100", earnedNano: "10" },
        { providerId: null, events: 2, spendNano: "250", earnedNano: "25" },
      ],
    })).toBe(35n);
  });
});
