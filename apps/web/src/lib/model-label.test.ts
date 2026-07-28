import { describe, expect, it } from "vitest";
import { modelLabel } from "./model-label";

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
});
