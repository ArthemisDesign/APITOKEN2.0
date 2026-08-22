import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { dailyProviderTotal, earningsShareTenths, providerLabel } from "./provider-breakdown";

describe("provider earnings breakdown", () => {
  it("knows every provider, including GLM, in stable Usage order", () => {
    const source = readFileSync(new URL("./provider-breakdown.tsx", import.meta.url), "utf8");
    expect(providerLabel("glm", "Historical")).toBe("GLM");
    expect(source).toContain('["anthropic", "openai", "google", "kimi", "glm"]');
  });

  it("keeps money aggregation in BigInt nanoUSD", () => {
    expect(earningsShareTenths("2500000000", 10_000_000_000n)).toBe(250);
    expect(dailyProviderTotal({
      date: "2026-08-22",
      providers: [
        { providerId: "openai", earnedNano: "1000000001", spendNano: "0", events: 1 },
        { providerId: "glm", earnedNano: "2000000002", spendNano: "0", events: 1 },
      ],
    })).toBe(3_000_000_003n);
  });
});
