import { describe, expect, it } from "vitest";
import { modelLabel } from "./format";

describe("modelLabel", () => {
  it("keeps the existing Claude labels", () => {
    expect(modelLabel("claude-opus-4-8")).toBe("Claude Opus 4.8");
  });

  it("does not present GPT models as Claude", () => {
    expect(modelLabel("gpt-5.6-sol")).toBe("GPT-5.6 Sol");
  });
});
