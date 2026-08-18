import { describe, expect, it } from "vitest";
import { earningsShareTenths, providerLabel } from "./provider-breakdown";

describe("provider earnings breakdown", () => {
  it("names the providers the pool actually serves", () => {
    expect(providerLabel("anthropic", "—")).toBe("Claude (Anthropic)");
    expect(providerLabel("openai", "—")).toBe("GPT (OpenAI)");
    expect(providerLabel("google", "—")).toBe("Gemini (Google)");
    expect(providerLabel("kimi", "—")).toBe("Kimi (Moonshot)");
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
});
