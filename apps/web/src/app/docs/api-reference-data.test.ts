import { describe, expect, it } from "vitest";
import {
  INTEGRATION_MODELS,
  ROUTER_BASE_URL,
  ROUTER_OPENAI_BASE_URL,
  type IntegrationLanguage,
  type IntegrationProvider,
} from "./integration-builder-data";
import { buildApiGuide, namespacedModelId, type ApiLanguage, type ApiStyle } from "./api-reference-data";

const providers: IntegrationProvider[] = ["anthropic", "openai", "gemini", "kimi"];
const apiStyles: ApiStyle[] = ["native", "openai-compatible"];
const apiLanguages: ApiLanguage[] = ["curl", "python", "typescript"];
const languages: IntegrationLanguage[] = ["en", "ru"];
const nativeEndpoints: Record<IntegrationProvider, string> = {
  anthropic: ROUTER_BASE_URL,
  openai: ROUTER_OPENAI_BASE_URL,
  gemini: ROUTER_BASE_URL,
  // KIMI speaks Anthropic Messages, so its native surface is the Anthropic endpoint.
  kimi: ROUTER_BASE_URL,
};

describe("API reference guide", () => {
  it("builds every provider, style, language, and UI-locale combination without a real key", () => {
    for (const provider of providers) {
      for (const apiStyle of apiStyles) {
        for (const apiLanguage of apiLanguages) {
          for (const language of languages) {
            const guide = buildApiGuide({ provider, apiStyle, apiLanguage, language });
            const withImageStep = provider === "openai" && apiStyle === "native";
            expect(guide.endpoint).toBe(apiStyle === "native" ? nativeEndpoints[provider] : ROUTER_OPENAI_BASE_URL);
            expect(guide.steps.length).toBe((apiLanguage === "curl" ? 2 : 3) + (withImageStep ? 2 : 0));
            expect(guide.steps.every((step) => step.code.trim().length > 0)).toBe(true);
            const request = guide.steps.at(withImageStep ? -3 : -1)!.code;
            expect(request).toContain(
              apiStyle === "native" ? INTEGRATION_MODELS[provider][0].id : namespacedModelId(provider, INTEGRATION_MODELS[provider][0].id),
            );
            expect(request).toContain("APITOKEN_API_KEY");
            expect(JSON.stringify(guide)).not.toContain("YOUR_SK_POOL_API_KEY");
          }
        }
      }
    }
  });

  it("uses the correct credential scheme per provider on the native lanes", () => {
    const anthropic = buildApiGuide({ provider: "anthropic", apiStyle: "native", apiLanguage: "curl", language: "en" });
    expect(anthropic.auth).toBe("x-api-key · anthropic-version");
    expect(anthropic.steps[1].code).toContain("x-api-key: $APITOKEN_API_KEY");
    expect(anthropic.steps[1].code).toContain("anthropic-version: 2023-06-01");
    expect(anthropic.steps[1].code).toContain(`${ROUTER_BASE_URL}/v1/messages`);

    const openai = buildApiGuide({ provider: "openai", apiStyle: "native", apiLanguage: "curl", language: "en" });
    expect(openai.auth).toBe("Authorization: Bearer");
    expect(openai.steps[1].code).toContain("Authorization: Bearer $APITOKEN_API_KEY");
    expect(openai.steps[1].code).toContain(`${ROUTER_OPENAI_BASE_URL}/responses`);

    const gemini = buildApiGuide({ provider: "gemini", apiStyle: "native", apiLanguage: "curl", language: "en" });
    expect(gemini.auth).toBe("x-goog-api-key");
    expect(gemini.steps[1].code).toContain("x-goog-api-key: $APITOKEN_API_KEY");
    expect(gemini.steps[1].code).toContain(`${ROUTER_BASE_URL}/v1beta/models/gemini-3.7-flash:generateContent`);
  });

  it("serves every provider through the OpenAI-compatible universal route", () => {
    for (const provider of providers) {
      const guide = buildApiGuide({ provider, apiStyle: "openai-compatible", apiLanguage: "curl", language: "en" });
      expect(guide.auth).toBe("Authorization: Bearer");
      expect(guide.steps[1].code).toContain(`${ROUTER_OPENAI_BASE_URL}/chat/completions`);
      expect(guide.steps[1].code).toContain(namespacedModelId(provider, INTEGRATION_MODELS[provider][0].id));
    }
    expect(namespacedModelId("anthropic", "claude-opus-5")).toBe("anthropic/claude-opus-5");
    expect(namespacedModelId("openai", "gpt-5.6-sol")).toBe("openai/gpt-5.6-sol");
    expect(namespacedModelId("gemini", "gemini-3.6-flash")).toBe("google/gemini-3.6-flash");
  });

  it("emits install steps with the official SDK packages", () => {
    const anthropicPython = buildApiGuide({ provider: "anthropic", apiStyle: "native", apiLanguage: "python", language: "en" });
    expect(anthropicPython.steps[1].code).toBe("pip install anthropic");
    const openaiTs = buildApiGuide({ provider: "openai", apiStyle: "native", apiLanguage: "typescript", language: "en" });
    expect(openaiTs.steps[1].code).toBe("npm install openai");
    const anthropicTs = buildApiGuide({ provider: "anthropic", apiStyle: "native", apiLanguage: "typescript", language: "ru" });
    expect(anthropicTs.steps[1].code).toBe("npm install @anthropic-ai/sdk");
    const geminiPython = buildApiGuide({ provider: "gemini", apiStyle: "native", apiLanguage: "python", language: "en" });
    expect(geminiPython.steps[1].code).toBe("pip install google-genai");
    const geminiTs = buildApiGuide({ provider: "gemini", apiStyle: "native", apiLanguage: "typescript", language: "en" });
    expect(geminiTs.steps[1].code).toBe("npm install @google/genai");
    const compatiblePython = buildApiGuide({ provider: "anthropic", apiStyle: "openai-compatible", apiLanguage: "python", language: "en" });
    expect(compatiblePython.steps[1].code).toBe("pip install openai");
    const compatibleTs = buildApiGuide({ provider: "gemini", apiStyle: "openai-compatible", apiLanguage: "typescript", language: "en" });
    expect(compatibleTs.steps[1].code).toBe("npm install openai");
  });

  it("keeps SDK examples parseable-looking and pointed at the unified endpoint", () => {
    const python = buildApiGuide({ provider: "anthropic", apiStyle: "native", apiLanguage: "python", language: "en" });
    expect(python.steps[2].code).toContain('from anthropic import Anthropic');
    expect(python.steps[2].code).toContain(`base_url="${ROUTER_BASE_URL}"`);
    const ts = buildApiGuide({ provider: "openai", apiStyle: "native", apiLanguage: "typescript", language: "en" });
    expect(ts.steps[2].code).toContain('import OpenAI from "openai"');
    expect(ts.steps[2].code).toContain(`baseURL: "${ROUTER_OPENAI_BASE_URL}"`);
    const geminiPython = buildApiGuide({ provider: "gemini", apiStyle: "native", apiLanguage: "python", language: "en" });
    expect(geminiPython.steps[2].code).toContain("from google import genai");
    expect(geminiPython.steps[2].code).toContain(`base_url="${ROUTER_BASE_URL}"`);
    const geminiTs = buildApiGuide({ provider: "gemini", apiStyle: "native", apiLanguage: "typescript", language: "en" });
    expect(geminiTs.steps[2].code).toContain('import { GoogleGenAI } from "@google/genai"');
    expect(geminiTs.steps[2].code).toContain(`baseUrl: "${ROUTER_BASE_URL}"`);
    const compatible = buildApiGuide({ provider: "anthropic", apiStyle: "openai-compatible", apiLanguage: "typescript", language: "en" });
    expect(compatible.steps[2].code).toContain('import OpenAI from "openai"');
    expect(compatible.steps[2].code).toContain("chat.completions.create");
    expect(compatible.steps[2].code).toContain("anthropic/claude-opus-5");
  });

  it("documents the GPT Image 2 route only on the OpenAI native lane", () => {
    for (const apiLanguage of ["curl", "python", "typescript"] as const) {
      const openai = buildApiGuide({ provider: "openai", apiStyle: "native", apiLanguage, language: "en" });
      const imageStep = openai.steps.find((step) => step.title.includes("GPT Image 2"))!;
      expect(imageStep.text).toContain("/v1/images/edits");
      expect(imageStep.code).toContain(ROUTER_OPENAI_BASE_URL);
      if (apiLanguage === "curl") {
        expect(imageStep.code).toContain(`${ROUTER_OPENAI_BASE_URL}/images/generations`);
      }
      expect(imageStep.code).toContain('"gpt-image-2"');
      expect(imageStep.code).toContain("APITOKEN_API_KEY");
      const maskStep = openai.steps.find((step) => step.title.includes("PNG mask"))!;
      expect(maskStep.text).toContain("input_image_mask");
      expect(maskStep.text).toContain("file_id");
      expect(maskStep.code).toContain("input_image_mask");
      expect(maskStep.code).toContain("data:image/png;base64");
      expect(maskStep.code).toContain("gpt-5.6-sol");

      const anthropic = buildApiGuide({ provider: "anthropic", apiStyle: "native", apiLanguage, language: "en" });
      expect(anthropic.steps.some((step) => step.code.includes("/images/generations"))).toBe(false);
      const compatible = buildApiGuide({ provider: "openai", apiStyle: "openai-compatible", apiLanguage, language: "en" });
      expect(compatible.steps.some((step) => step.code.includes("/images/generations"))).toBe(false);
    }
  });
});
