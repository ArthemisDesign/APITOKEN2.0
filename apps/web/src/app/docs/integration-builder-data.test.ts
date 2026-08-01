import { describe, expect, it } from "vitest";
import { ANTHROPIC_BASE_URL as CANONICAL_ANTHROPIC_BASE_URL, GEMINI_BASE_URL as CANONICAL_GEMINI_BASE_URL, OPENAI_BASE_URL as CANONICAL_OPENAI_BASE_URL, claudeModels, geminiModels, openaiModels } from "@/lib/models";
import {
  ANTHROPIC_BASE_URL,
  GEMINI_BASE_URL,
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
const providers: IntegrationProvider[] = ["anthropic", "openai", "gemini"];
const tools = Object.keys(TOOL_COMPATIBILITY) as IntegrationTool[];
const providerEndpoints: Record<IntegrationProvider, string> = {
  anthropic: ANTHROPIC_BASE_URL,
  openai: OPENAI_BASE_URL,
  gemini: GEMINI_BASE_URL,
};
const providerTitleNames: Record<IntegrationProvider, string> = {
  anthropic: "Claude",
  openai: "GPT",
  gemini: "Gemini",
};

describe("integration builder guide", () => {
  it("keeps the browser catalog in parity with the canonical model registry", () => {
    expect(ANTHROPIC_BASE_URL).toBe(CANONICAL_ANTHROPIC_BASE_URL);
    expect(OPENAI_BASE_URL).toBe(CANONICAL_OPENAI_BASE_URL);
    expect(GEMINI_BASE_URL).toBe(CANONICAL_GEMINI_BASE_URL);
    expect(INTEGRATION_MODELS.anthropic.map(({ id }) => id)).toEqual(claudeModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.openai.map(({ id }) => id)).toEqual(openaiModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.gemini.map(({ id }) => id)).toEqual(geminiModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.anthropic.every(({ name }) => name.length > 0)).toBe(true);
    expect(INTEGRATION_MODELS.openai.every(({ name }) => name.length > 0)).toBe(true);
    expect(INTEGRATION_MODELS.gemini.every(({ name }) => name.length > 0)).toBe(true);
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
            expect(guide.endpoint).toBe(providerEndpoints[provider]);
            expect(guide.title).toContain(providerTitleNames[provider]);
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

    // Antigravity has no bring-your-own-endpoint, so it can never build a guide.
    expect(() => buildIntegrationGuide({
      provider: "gemini",
      tool: "antigravity",
      os: "unix",
      modelId: INTEGRATION_MODELS.gemini[0].id,
      language: "en",
    })).toThrow("Antigravity does not support gemini");
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

    const geminiOpenCode = buildIntegrationGuide({ provider: "gemini", tool: "opencode", os: "unix", modelId: "gemini-3.6-flash", language: "en" });
    const geminiOpenCodeConfig = JSON.parse(geminiOpenCode.steps[0].code) as { provider: { apitoken: { npm: string; options: { baseURL: string; apiKey: string } } } };
    expect(geminiOpenCodeConfig.provider.apitoken.npm).toBe("@ai-sdk/google");
    expect(geminiOpenCodeConfig.provider.apitoken.options.baseURL).toBe(`${GEMINI_BASE_URL}/v1beta`);
    expect(geminiOpenCodeConfig.provider.apitoken.options.apiKey).toBe("{env:APITOKEN_API_KEY}");

    const geminiPi = buildIntegrationGuide({ provider: "gemini", tool: "pi", os: "unix", modelId: "gemini-3.6-flash", language: "en" });
    const geminiPiConfig = JSON.parse(geminiPi.steps[0].code) as { providers: { apitoken: { baseUrl: string; api: string } } };
    expect(geminiPiConfig.providers.apitoken.baseUrl).toBe(GEMINI_BASE_URL);
    expect(geminiPiConfig.providers.apitoken.api).toBe("google-generative-ai");
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

    const geminiCli = buildIntegrationGuide({ provider: "gemini", tool: "gemini-cli", os: "unix", modelId: "gemini-3.6-flash", language: "en" });
    expect(geminiCli.endpoint).toBe(GEMINI_BASE_URL);
    expect(geminiCli.steps[0].code).toContain(`export GOOGLE_GEMINI_BASE_URL="${GEMINI_BASE_URL}"`);
    expect(geminiCli.steps[0].code).toContain('export GEMINI_API_KEY="sk-pool-•••"');
    expect(geminiCli.steps[1].code).toBe("gemini --model gemini-3.6-flash");
    expect(geminiCli.requirement).toContain("/auth");
    // The two failure modes the user hit live: doubled /v1beta and the default
    // auto model — the guide must warn about both explicitly.
    expect(geminiCli.requirement).toContain("without /v1beta");
    expect(geminiCli.requirement).toContain("not available");
    expect(geminiCli.steps[1].text).toContain("auto");

    const geminiCliWindows = buildIntegrationGuide({ provider: "gemini", tool: "gemini-cli", os: "powershell", modelId: "gemini-3.6-flash", language: "en" });
    expect(geminiCliWindows.steps[0].code).toContain(`$env:GOOGLE_GEMINI_BASE_URL = "${GEMINI_BASE_URL}"`);
  });
});
