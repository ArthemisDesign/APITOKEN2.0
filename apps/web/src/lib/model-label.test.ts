import { describe, expect, it } from "vitest";
import { modelLabel, modelProvider } from "./model-label";

describe("modelLabel", () => {
  it("formats Claude model IDs without changing existing dashboard labels", () => {
    expect(modelLabel("claude-opus-4-8")).toBe("Claude Opus 4.8");
    expect(modelLabel("claude-3-5-sonnet-20241022")).toBe("Claude Sonnet 3.5");
  });

  it("formats GPT model IDs without a Claude prefix", () => {
    expect(modelLabel("gpt-5.6-sol")).toBe("GPT 5.6 Sol");
    expect(modelLabel("gpt-5-6-terra")).toBe("GPT 5.6 Terra");
  });

  it("uses a provider-neutral fallback for other model families", () => {
    expect(modelLabel("gemini-2.5-pro")).toBe("Gemini 2.5 Pro");
  });

  it("labels both KIMI alias shapes", () => {
    expect(modelLabel("k3")).toBe("K3");
    expect(modelLabel("k3-256k")).toBe("K3 256k");
    expect(modelLabel("kimi-for-coding")).toBe("Kimi For Coding");
  });
});

describe("modelProvider", () => {
  it("groups both KIMI alias shapes under one provider, namespaced or bare", () => {
    // `kimi-for-coding…` and the bare `k3` family are the same provider. Matching only the
    // first spelling would send half the published catalog to the neutral bucket.
    for (const id of ["k3", "k3[1m]", "k3-256k", "kimi-for-coding", "kimi-for-coding-highspeed"]) {
      expect(modelProvider(id), id).toBe("kimi");
      expect(modelProvider(`kimi/${id}`), id).toBe("kimi");
    }
  });

  it("leaves the existing families and the neutral bucket unchanged", () => {
    expect(modelProvider("claude-opus-5")).toBe("anthropic");
    expect(modelProvider("gpt-5.6-sol")).toBe("openai");
    expect(modelProvider("gemini-3.6-flash")).toBe("gemini");
    expect(modelProvider("command-x")).toBe("other");
  });
});
