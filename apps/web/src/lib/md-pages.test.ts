import { describe, expect, it } from "vitest";
import { buildAgentSetupMarkdown } from "./md-pages";
import { claudeModels, geminiModels, openaiModels } from "./models";

describe("AI agent connection guide", () => {
  it("covers all three API surfaces, operating systems, verification, and every catalog model", () => {
    const guide = buildAgentSetupMarkdown();

    expect(guide).toContain("https://api.apitoken.sale/v1/messages");
    expect(guide).toContain("https://openai.api.apitoken.sale/v1/models");
    expect(guide).toContain("https://gemini.api.apitoken.sale/v1beta/models");
    expect(guide).toContain("x-goog-api-key");
    expect(guide).toContain("Windows PowerShell");
    expect(guide).toContain("macOS and Linux");
    expect(guide).toContain("Diagnostic decision tree");
    expect(guide).toContain("API key: REDACTED");

    for (const model of [...claudeModels, ...openaiModels, ...geminiModels]) {
      expect(guide).toContain(`\`${model.id}\``);
    }
  });
});
