import { describe, expect, it } from "vitest";
import { ANTHROPIC_BASE_URL as CANONICAL_ANTHROPIC_BASE_URL, GEMINI_BASE_URL as CANONICAL_GEMINI_BASE_URL, OPENAI_BASE_URL as CANONICAL_OPENAI_BASE_URL, ROUTER_BASE_URL as CANONICAL_ROUTER_BASE_URL, ROUTER_OPENAI_BASE_URL as CANONICAL_ROUTER_OPENAI_BASE_URL, claudeModels, geminiModels, openaiModels } from "@/lib/models";
import {
  ANTHROPIC_BASE_URL,
  GEMINI_BASE_URL,
  INTEGRATION_MODELS,
  MODEL_WINDOWS,
  OPENAI_BASE_URL,
  OPENCODE_INSTALLER_URL,
  ROUTER_BASE_URL,
  ROUTER_OPENAI_BASE_URL,
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
const providers: IntegrationProvider[] = ["anthropic", "openai", "gemini", "kimi"];
const tools = Object.keys(TOOL_COMPATIBILITY) as IntegrationTool[];
const providerEndpoints: Record<IntegrationProvider, string> = {
  anthropic: ROUTER_BASE_URL,
  openai: ROUTER_OPENAI_BASE_URL,
  gemini: ROUTER_BASE_URL,
  kimi: ROUTER_BASE_URL,
};
const providerTitleNames: Record<IntegrationProvider, string> = {
  anthropic: "Claude",
  openai: "GPT",
  gemini: "Gemini",
  kimi: "Kimi",
};

describe("integration builder guide", () => {
  it("keeps the browser catalog in parity with the canonical model registry", () => {
    expect(ROUTER_BASE_URL).toBe(CANONICAL_ROUTER_BASE_URL);
    expect(ROUTER_OPENAI_BASE_URL).toBe(CANONICAL_ROUTER_OPENAI_BASE_URL);
    expect(ANTHROPIC_BASE_URL).toBe(CANONICAL_ANTHROPIC_BASE_URL);
    expect(OPENAI_BASE_URL).toBe(CANONICAL_OPENAI_BASE_URL);
    expect(GEMINI_BASE_URL).toBe(CANONICAL_GEMINI_BASE_URL);
    expect(INTEGRATION_MODELS.anthropic.map(({ id }) => id)).toEqual(claudeModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.openai.map(({ id }) => id)).toEqual(openaiModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.gemini.map(({ id }) => id)).toEqual(geminiModels.map(({ id }) => id));
    expect(INTEGRATION_MODELS.anthropic.every(({ name }) => name.length > 0)).toBe(true);
    expect(INTEGRATION_MODELS.openai.every(({ name }) => name.length > 0)).toBe(true);
    expect(INTEGRATION_MODELS.gemini.every(({ name }) => name.length > 0)).toBe(true);
    // KIMI has no entry in the SEO registry (it is not published on the marketing site), so its
    // catalog is pinned here instead: exactly the five aliases the router advertises, and never
    // an official Open Platform id, which the gateway refuses on the wire.
    expect(INTEGRATION_MODELS.kimi.map(({ id }) => id)).toEqual([
      "k3", "k3[1m]", "k3-256k", "kimi-for-coding", "kimi-for-coding-highspeed",
    ]);
    expect(INTEGRATION_MODELS.kimi.every(({ name }) => name.startsWith("Kimi"))).toBe(true);
  });

  it("routes KIMI over the Anthropic protocol under its own namespace", () => {
    const openCode = buildIntegrationGuide({ provider: "kimi", tool: "opencode", os: "unix", modelId: "k3", language: "en" });
    // The namespace is KIMI's own — unlike Gemini, whose catalog namespace is `google`.
    expect(openCode.steps[1].code).toContain("apitoken/kimi/k3");
    const config = JSON.parse(openCode.steps[2].code) as { provider: { apitoken: { npm: string; options: { baseURL: string } } } };
    // Anthropic Messages, not the OpenAI-compatible adapter an `else` branch would have chosen.
    expect(config.provider.apitoken.npm).toBe("@ai-sdk/anthropic");
    expect(config.provider.apitoken.options.baseURL).toBe(`${ROUTER_BASE_URL}/v1`);

    const pi = buildIntegrationGuide({ provider: "kimi", tool: "pi", os: "unix", modelId: "k3", language: "en" });
    const piConfig = JSON.parse(pi.steps[0].code) as { providers: { apitoken: { baseUrl: string; api: string } } };
    expect(piConfig.providers.apitoken.api).toBe("anthropic-messages");
    expect(piConfig.providers.apitoken.baseUrl).toBe(ROUTER_BASE_URL);

    expect(isToolCompatible("claude-code", "kimi")).toBe(true);
  });

  it("pins every Claude Code model tier on KIMI and matches the window to the alias", () => {
    const oneM = buildIntegrationGuide({ provider: "kimi", tool: "claude-code", os: "unix", modelId: "k3[1m]", language: "en" });
    const env = oneM.steps[0].code;
    // Claude Code resolves a model per tier. An unpinned tier fails only in subagents and
    // background tasks, which reads as a gateway fault rather than a configuration gap.
    for (const name of [
      "ANTHROPIC_MODEL",
      "ANTHROPIC_DEFAULT_OPUS_MODEL",
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      "ANTHROPIC_DEFAULT_HAIKU_MODEL",
      "CLAUDE_CODE_SUBAGENT_MODEL",
    ]) {
      expect(env).toContain(`${name}="k3[1m]"`);
    }
    // The bracket alias is the whole reason the 1M window is reachable from this agent.
    expect(env).toContain("CLAUDE_CODE_MAX_CONTEXT_TOKENS=\"1048576\"");

    // A 256k alias must not claim the 1M window, or the agent would compact far too late.
    const short = buildIntegrationGuide({ provider: "kimi", tool: "claude-code", os: "unix", modelId: "k3-256k", language: "en" });
    expect(short.steps[0].code).toContain('ANTHROPIC_MODEL="k3-256k"');
    expect(short.steps[0].code).not.toContain("CLAUDE_CODE_MAX_CONTEXT_TOKENS");

    // Anthropic's own guide must stay exactly as it was: no tier pinning, no window override.
    const anthropic = buildIntegrationGuide({ provider: "anthropic", tool: "claude-code", os: "unix", modelId: "claude-opus-5", language: "en" });
    expect(anthropic.steps[0].code).not.toContain("ANTHROPIC_DEFAULT_OPUS_MODEL");
    expect(anthropic.steps[0].code).not.toContain("CLAUDE_CODE_SUBAGENT_MODEL");
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
            // Kimi Code speaks the universal OpenAI-compatible lane for every provider, so its
            // endpoint is the `/v1` one even when the provider's native lane is elsewhere.
            const expectedEndpoint = tool === "kimi-code"
              ? ROUTER_OPENAI_BASE_URL
              : providerEndpoints[provider];
            expect(guide.endpoint).toBe(expectedEndpoint);
            expect(guide.title).toContain(providerTitleNames[provider]);
            expect(guide.steps.every((step) => step.code.trim().length > 0)).toBe(true);
            expect(JSON.stringify(guide)).not.toContain("YOUR_SK_POOL_API_KEY");
          }
        }
      }
    }
  });

  it("emits a Kimi Code config that names our lane, the wire id and a reviewed window", () => {
    const guide = buildIntegrationGuide({ provider: "kimi", tool: "kimi-code", os: "unix", modelId: "k3", language: "en" });
    const config = guide.steps[1].code;
    // An ordinary OpenAI-compatible provider entry — this is why one entry reaches the whole
    // catalogue and not just KIMI.
    expect(config).toContain('type = "openai"');
    expect(config).toContain(`base_url = "${ROUTER_OPENAI_BASE_URL}"`);
    // The alias on the left is local; what goes on the wire is the namespaced catalogue id.
    expect(config).toContain('[models."apitoken/k3"]');
    expect(config).toContain('model = "kimi/k3"');
    expect(config).toContain("max_context_size = 1048576");
    // The harness picks a model from these blocks, so the config must declare the provider's
    // whole catalogue — one block would leave it with a single choice.
    for (const entry of INTEGRATION_MODELS.kimi) {
      expect(config, entry.id).toContain(`[models."apitoken/${entry.id}"]`);
    }
    expect(config).toContain('default_model = "apitoken/k3"');
    // Kimi Code refuses to read credentials from the shell, so the file holds the key and must
    // be locked down — dropping that line would publish a world-readable secret.
    expect(config).toContain("chmod 600 ~/.kimi-code/config.toml");
    expect(guide.securityNote).toContain("config.toml");

    // Gemini keeps its `google/` catalogue namespace here, exactly as in OpenCode.
    const gemini = buildIntegrationGuide({ provider: "gemini", tool: "kimi-code", os: "unix", modelId: "gemini-3.6-flash", language: "en" });
    expect(gemini.steps[1].code).toContain('model = "google/gemini-3.6-flash"');
    // GPT bills a 400K window but caps one request at 272K; compaction must respect the smaller.
    const gpt = buildIntegrationGuide({ provider: "openai", tool: "kimi-code", os: "unix", modelId: "gpt-5.6-sol", language: "en" });
    expect(gpt.steps[1].code).toContain("max_context_size = 400000");
    expect(gpt.steps[1].code).toContain("max_input_size = 272000");

    // Selecting Claude must put every Claude model in the harness, on our key and our lane.
    const claude = buildIntegrationGuide({ provider: "anthropic", tool: "kimi-code", os: "unix", modelId: "claude-opus-5", language: "en" });
    for (const entry of INTEGRATION_MODELS.anthropic) {
      expect(claude.steps[1].code, entry.id).toContain(`model = "anthropic/${entry.id}"`);
    }
    expect(claude.steps[1].code).toContain('default_model = "apitoken/claude-opus-5"');
    // One provider entry carries them all: the model id selects the provider, not the base URL.
    expect(claude.steps[1].code.match(/\[providers\./g)).toHaveLength(1);
  });

  it("has a reviewed context window for every published model", () => {
    // `max_context_size` is required by Kimi Code and decides when it compacts, so a model may
    // not reach the builder without one — and a stale entry for a dropped model is dead weight.
    const published = Object.values(INTEGRATION_MODELS).flatMap((models) => models.map(({ id }) => id));
    expect(Object.keys(MODEL_WINDOWS).sort()).toEqual([...published].sort());
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
    expect(openCode.steps[0].code).toBe(`curl -fsSL ${OPENCODE_INSTALLER_URL} | bash`);
    expect(openCode.steps[1].code).toContain("apitoken/openai/gpt-5.6-sol");
    const openCodeConfig = JSON.parse(openCode.steps[2].code) as { provider: { apitoken: { options: { baseURL: string; apiKey: string }; models: Record<string, unknown> } } };
    expect(openCodeConfig.provider.apitoken.options.baseURL).toBe(ROUTER_OPENAI_BASE_URL);
    expect(openCodeConfig.provider.apitoken.options.apiKey).toBe("{env:APITOKEN_API_KEY}");
    expect(openCodeConfig.provider.apitoken.models["gpt-5.6-sol"]).toBeTruthy();

    const openCodeWindows = buildIntegrationGuide({ provider: "openai", tool: "opencode", os: "powershell", modelId: "gpt-5.6-sol", language: "en" });
    const openCodeWindowsConfig = JSON.parse(openCodeWindows.steps[0].code) as { provider: { apitoken: { options: { apiKey: string } } } };
    expect(openCodeWindowsConfig.provider.apitoken.options.apiKey).toBe("{env:APITOKEN_API_KEY}");
    expect(openCodeWindows.steps[1].code).toContain('$env:APITOKEN_API_KEY = "sk-pool-•••"');

    const pi = buildIntegrationGuide({ provider: "openai", tool: "pi", os: "unix", modelId: "gpt-5.6-sol", language: "en" });
    const piConfig = JSON.parse(pi.steps[0].code) as { providers: { apitoken: { baseUrl: string; api: string; apiKey: string; models: Array<{ id: string }> } } };
    expect(piConfig.providers.apitoken.baseUrl).toBe(ROUTER_OPENAI_BASE_URL);
    expect(piConfig.providers.apitoken.api).toBe("openai-completions");
    expect(piConfig.providers.apitoken.apiKey).toBe("$APITOKEN_API_KEY");
    expect(piConfig.providers.apitoken.models[0].id).toBe("gpt-5.6-sol");

    const geminiOpenCode = buildIntegrationGuide({ provider: "gemini", tool: "opencode", os: "unix", modelId: "gemini-3.6-flash", language: "en" });
    expect(geminiOpenCode.steps[1].code).toContain("apitoken/google/gemini-3.6-flash");
    const geminiOpenCodeConfig = JSON.parse(geminiOpenCode.steps[2].code) as { provider: { apitoken: { npm: string; options: { baseURL: string; apiKey: string } } } };
    expect(geminiOpenCodeConfig.provider.apitoken.npm).toBe("@ai-sdk/google");
    expect(geminiOpenCodeConfig.provider.apitoken.options.baseURL).toBe(`${ROUTER_BASE_URL}/v1beta`);
    expect(geminiOpenCodeConfig.provider.apitoken.options.apiKey).toBe("{env:APITOKEN_API_KEY}");

    const geminiPi = buildIntegrationGuide({ provider: "gemini", tool: "pi", os: "unix", modelId: "gemini-3.6-flash", language: "en" });
    const geminiPiConfig = JSON.parse(geminiPi.steps[0].code) as { providers: { apitoken: { baseUrl: string; api: string } } };
    expect(geminiPiConfig.providers.apitoken.baseUrl).toBe(ROUTER_BASE_URL);
    expect(geminiPiConfig.providers.apitoken.api).toBe("google-generative-ai");
  });

  it("keeps shell syntax and provider-specific wire formats correct", () => {
    const claudeWindows = buildIntegrationGuide({ provider: "anthropic", tool: "claude-code", os: "powershell", modelId: "claude-opus-4-8", language: "en" });
    expect(claudeWindows.steps[0].code).toContain('$env:ANTHROPIC_BASE_URL = "https://router.apitoken.sale"');
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
    expect(geminiCli.endpoint).toBe(ROUTER_BASE_URL);
    expect(geminiCli.steps[0].code).toContain(`export GOOGLE_GEMINI_BASE_URL="${ROUTER_BASE_URL}"`);
    expect(geminiCli.steps[0].code).toContain('export GEMINI_API_KEY="sk-pool-•••"');
    expect(geminiCli.steps[1].code).toBe("gemini --model gemini-3.6-flash");
    expect(geminiCli.requirement).toContain("/auth");
    // The two failure modes the user hit live: doubled /v1beta and the default
    // auto model — the guide must warn about both explicitly.
    expect(geminiCli.requirement).toContain("without /v1beta");
    expect(geminiCli.requirement).toContain("not available");
    expect(geminiCli.steps[1].text).toContain("auto");

    const geminiCliWindows = buildIntegrationGuide({ provider: "gemini", tool: "gemini-cli", os: "powershell", modelId: "gemini-3.6-flash", language: "en" });
    expect(geminiCliWindows.steps[0].code).toContain(`$env:GOOGLE_GEMINI_BASE_URL = "${ROUTER_BASE_URL}"`);
  });
});
