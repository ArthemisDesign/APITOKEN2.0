import { describe, expect, it } from "vitest";
import {
  ANTHROPIC_BASE_URL,
  INTEGRATION_MODELS,
  OPENAI_BASE_URL,
  type IntegrationLanguage,
  type IntegrationProvider,
} from "./integration-builder-data";
import { buildApiGuide, type ApiLanguage } from "./api-reference-data";

const providers: IntegrationProvider[] = ["anthropic", "openai"];
const apiLanguages: ApiLanguage[] = ["curl", "python", "typescript"];
const languages: IntegrationLanguage[] = ["en", "ru"];

describe("API reference guide", () => {
  it("builds every provider, language, and UI-locale combination without a real key", () => {
    for (const provider of providers) {
      for (const apiLanguage of apiLanguages) {
        for (const language of languages) {
          const guide = buildApiGuide({ provider, apiLanguage, language });
          expect(guide.endpoint).toBe(provider === "anthropic" ? ANTHROPIC_BASE_URL : OPENAI_BASE_URL);
          expect(guide.steps.length).toBe(apiLanguage === "curl" ? 2 : 3);
          expect(guide.steps.every((step) => step.code.trim().length > 0)).toBe(true);
          const request = guide.steps.at(-1)!.code;
          expect(request).toContain(INTEGRATION_MODELS[provider][0].id);
          expect(request).toContain("APITOKEN_API_KEY");
          expect(JSON.stringify(guide)).not.toContain("YOUR_SK_POOL_API_KEY");
        }
      }
    }
  });

  it("uses the correct credential scheme per provider", () => {
    const anthropic = buildApiGuide({ provider: "anthropic", apiLanguage: "curl", language: "en" });
    expect(anthropic.auth).toBe("x-api-key · anthropic-version");
    expect(anthropic.steps[1].code).toContain("x-api-key: $APITOKEN_API_KEY");
    expect(anthropic.steps[1].code).toContain("anthropic-version: 2023-06-01");
    expect(anthropic.steps[1].code).toContain(`${ANTHROPIC_BASE_URL}/v1/messages`);

    const openai = buildApiGuide({ provider: "openai", apiLanguage: "curl", language: "en" });
    expect(openai.auth).toBe("Authorization: Bearer");
    expect(openai.steps[1].code).toContain("Authorization: Bearer $APITOKEN_API_KEY");
    expect(openai.steps[1].code).toContain(`${OPENAI_BASE_URL}/responses`);
  });

  it("emits install steps with the official SDK packages", () => {
    const anthropicPython = buildApiGuide({ provider: "anthropic", apiLanguage: "python", language: "en" });
    expect(anthropicPython.steps[1].code).toBe("pip install anthropic");
    const openaiTs = buildApiGuide({ provider: "openai", apiLanguage: "typescript", language: "en" });
    expect(openaiTs.steps[1].code).toBe("npm install openai");
    const anthropicTs = buildApiGuide({ provider: "anthropic", apiLanguage: "typescript", language: "ru" });
    expect(anthropicTs.steps[1].code).toBe("npm install @anthropic-ai/sdk");
  });

  it("keeps SDK examples parseable-looking and pointed at apiToken.sale", () => {
    const python = buildApiGuide({ provider: "anthropic", apiLanguage: "python", language: "en" });
    expect(python.steps[2].code).toContain('from anthropic import Anthropic');
    expect(python.steps[2].code).toContain(`base_url="${ANTHROPIC_BASE_URL}"`);
    const ts = buildApiGuide({ provider: "openai", apiLanguage: "typescript", language: "en" });
    expect(ts.steps[2].code).toContain('import OpenAI from "openai"');
    expect(ts.steps[2].code).toContain(`baseURL: "${OPENAI_BASE_URL}"`);
  });
});
