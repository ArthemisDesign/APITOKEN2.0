import { describe, expect, it } from "vitest";
import { modelLabel } from "./format";

describe("modelLabel", () => {
  it("keeps the existing Claude labels", () => {
    expect(modelLabel("claude-opus-4-8")).toBe("Claude Opus 4.8");
    expect(modelLabel("legacy-sonnet-4-6", "anthropic")).toBe("Claude Legacy Sonnet 4.6");
  });

  it("does not present GPT models as Claude", () => {
    expect(modelLabel("gpt-5.6-sol")).toBe("GPT-5.6 Sol");
  });

  it("does not present Gemini models as Claude", () => {
    expect(modelLabel("gemini-3.5-flash", "google")).toBe("Gemini 3.5 Flash");
    expect(modelLabel("gemini-3-6-flash", "gemini")).toBe("Gemini 3.6 Flash");
  });

  it("uses a provider-neutral fallback for unknown model families", () => {
    expect(modelLabel("mistral-large-2", "openai")).toBe("Mistral Large 2");
  });
});
