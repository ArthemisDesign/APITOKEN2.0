import { describe, expect, it } from "vitest";
import { ANTHROPIC_BASE_URL as CANONICAL_ANTHROPIC_BASE_URL, OPENAI_BASE_URL as CANONICAL_OPENAI_BASE_URL, claudeModels, openaiModels } from "@/lib/models";
import {
  ANTHROPIC_BASE_URL,
  INTEGRATION_MODELS,
  OPENAI_BASE_URL,
  TOOL_COMPATIBILITY,
  buildIntegrationGuide,
  isToolCompatible,
  type IntegrationLanguage,
  type IntegrationOs,
  type IntegrationProvider,
  type IntegrationTool,
} from "./integration-builder-data";

const operatingSystems: IntegrationOs[] = ["unix", "powershell", "cmd"];
const languages: IntegrationLanguage[] = ["en", "ru"];
const providers: IntegrationProvider[] = ["anthropic", "openai"];
const tools = Object.keys(TOOL_COMPATIBILITY) as IntegrationTool[];

describe("integration builder guide", () => {
  it("keeps the browser catalog in parity with the canonical model registry", () => {
    expect(ANTHROPIC_BASE_URL).toBe(CANONICAL_ANTHROPIC_BASE_URL);
    expect(OPENAI_BASE_URL).toBe(CANONICAL_OPENAI_BASE_URL);
    expect(INTEGRATION_MODELS.anthropic.map(({ id }) => id)).toEqual(claudeModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.openai.map(({ id }) => id)).toEqual(openaiModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.anthropic.every(({ name }) => name.length > 0)).toBe(true);
    expect(INTEGRATION_MODELS.openai.every(({ name }) => name.length > 0)).toBe(true);
  });

  it("builds every compatible provider, tool, OS, and language combination", () => {
    for (const provider of providers) {
      for (const tool of tools) {
        for (const os of operatingSystems) {
          for (const language of languages) {
            if (!isToolCompatible(tool, provider)) continue;
            const modelId = INTEGRATION_MODELS[provider][0].id;
            const guide = buildIntegrationGuide({ provider, tool, os, modelId, language });

            expect(guide.steps).toHaveLength(3);
            expect(guide.endpoint).toBe(provider === "anthropic" ? ANTHROPIC_BASE_URL : OPENAI_BASE_URL);
            expect(guide.title).toContain(modelId.includes("claude") ? "Claude" : "GPT");
            expect(guide.steps.every((step) => step.code.trim().length > 0)).toBe(true);
            expect(JSON.stringify(guide)).not.toContain("YOUR_SK_POOL_API_KEY");
          }
        }
      }
    }
  });

  it("rejects an incompatible provider/tool pair and an unknown model", () => {
    expect(() => buildIntegrationGuide({
      provider: "anthropic",
      tool: "codex",
      os: "unix",
      modelId: INTEGRATION_MODELS.anthropic[0].id,
      language: "en",
    })).toThrow("Codex does not support anthropic");

    expect(() => buildIntegrationGuide({
      provider: "openai",
      tool: "codex",
      os: "unix",
      modelId: "not-a-real-model",
      language: "en",
    })).toThrow("Unknown openai model");
  });

  it("emits parseable OpenCode and Pi configs without embedding a secret", () => {
    const openCode = buildIntegrationGuide({ provider: "openai", tool: "opencode", os: "unix", modelId: "gpt-5.6-sol", language: "en" });
    const openCodeConfig = JSON.parse(openCode.steps[0].code) as { provider: { apitoken: { options: { baseURL: string; apiKey: string }; models: Record<string, unknown> } } };
    expect(openCodeConfig.provider.apitoken.options.baseURL).toBe(OPENAI_BASE_URL);
    expect(openCodeConfig.provider.apitoken.options.apiKey).toBe("{env:APITOKEN_API_KEY}");
    expect(openCodeConfig.provider.apitoken.models["gpt-5.6-sol"]).toBeTruthy();
    expect(openCode.steps[1].code).toContain("export APITOKEN_API_KEY=\"sk-pool-•••\"");

    const pi = buildIntegrationGuide({ provider: "openai", tool: "pi", os: "unix", modelId: "gpt-5.6-sol", language: "en" });
    const piConfig = JSON.parse(pi.steps[0].code) as { providers: { apitoken: { baseUrl: string; api: string; apiKey: string; models: Array<{ id: string }> } } };
    expect(piConfig.providers.apitoken.baseUrl).toBe(OPENAI_BASE_URL);
    expect(piConfig.providers.apitoken.api).toBe("openai-completions");
    expect(piConfig.providers.apitoken.apiKey).toBe("$APITOKEN_API_KEY");
    expect(piConfig.providers.apitoken.models[0].id).toBe("gpt-5.6-sol");
  });

  it("keeps shell syntax and provider-specific wire formats correct", () => {
    const claudeWindows = buildIntegrationGuide({ provider: "anthropic", tool: "claude-code", os: "powershell", modelId: "claude-opus-4-8", language: "en" });
    expect(claudeWindows.steps[0].code).toContain('$env:ANTHROPIC_BASE_URL = "https://api.apitoken.sale"');
    expect(claudeWindows.steps[0].code).toContain("Remove-Item Env:ANTHROPIC_AUTH_TOKEN");

    const codexWindows = buildIntegrationGuide({ provider: "openai", tool: "codex", os: "cmd", modelId: "gpt-5.6-sol", language: "en" });
    expect(codexWindows.steps[0].codeLabel).toContain("%USERPROFILE%\\.codex");
    expect(codexWindows.steps[0].code).toContain('wire_api = "responses"');
    expect(codexWindows.steps[1].code).toContain("set \"APITOKEN_API_KEY=sk-pool-•••\"");

    const hermes = buildIntegrationGuide({ provider: "openai", tool: "hermes", os: "unix", modelId: "gpt-5.6-sol", language: "en" });
    expect(hermes.steps[1].code).toContain("API mode: Chat Completions");
    expect(hermes.steps[1].code).not.toContain("Responses API");
    expect(hermes.securityNote).toContain("~/.hermes");
  });
});
