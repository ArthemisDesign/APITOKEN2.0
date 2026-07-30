import { describe, expect, it } from "vitest";
import {
  buildAgentHandoff,
  buildClaudeCodeCommands,
  buildCodexCommands,
  CLAUDE_API_BASE_URL,
  CLAUDE_MESSAGES_URL,
  OPENAI_API_BASE_URL,
  OPENAI_RESPONSES_URL,
  publicDocsUrl,
} from "./claude-connection";

describe("Claude connection handoff", () => {
  it("builds a ready-to-paste Claude Code terminal setup", () => {
    expect(buildClaudeCodeCommands("sk-pool-test-secret")).toBe(`echo 'export ANTHROPIC_BASE_URL="https://api.apitoken.sale"' >> ~/.zshrc
echo 'export ANTHROPIC_API_KEY="sk-pool-test-secret"' >> ~/.zshrc
source ~/.zshrc
claude`);
    expect(buildClaudeCodeCommands()).toContain("YOUR_SK_POOL_API_KEY");
  });

  it("keeps the persistent zsh setup short and readable", () => {
    const commands = buildClaudeCodeCommands("sk-pool-test-secret");

    expect(commands.split("\n")).toHaveLength(4);
    expect(commands).toContain(">> ~/.zshrc");
    expect(commands).toContain("source ~/.zshrc");
  });

  it("expands the dashboard docs route to a shareable public URL", () => {
    expect(publicDocsUrl("/docs")).toBe("https://apitoken.sale/docs");
    expect(publicDocsUrl("https://docs.example.com/claude")).toBe("https://docs.example.com/claude");
  });

  it("configures both future and current terminals on Windows", () => {
    for (const shell of ["powershell", "cmd"] as const) {
      const commands = buildClaudeCodeCommands("sk-pool-test-secret", shell);
      // setx alone would leave the current window unconfigured and the final `claude` line broken.
      expect(commands).toContain('setx ANTHROPIC_BASE_URL "https://api.apitoken.sale"');
      expect(commands).toContain('setx ANTHROPIC_API_KEY "sk-pool-test-secret"');
      expect(commands.trim().endsWith("claude")).toBe(true);
      expect(commands).not.toContain("~/.zshrc");
    }
    expect(buildClaudeCodeCommands("sk-pool-test-secret", "powershell")).toContain('$env:ANTHROPIC_API_KEY = "sk-pool-test-secret"');
    expect(buildClaudeCodeCommands("sk-pool-test-secret", "cmd")).toContain("set ANTHROPIC_API_KEY=sk-pool-test-secret");
  });
});

describe("Codex connection setup", () => {
  it("writes a named model_providers profile and runs it explicitly", () => {
    const commands = buildCodexCommands("sk-pool-test-secret");

    expect(commands).toContain("~/.codex/apitoken.config.toml");
    expect(commands).toContain(`base_url = "${OPENAI_API_BASE_URL}"`);
    expect(commands).toContain('wire_api = "responses"');
    expect(commands).toContain('env_key = "APITOKEN_API_KEY"');
    expect(commands).toContain('model = "gpt-5.6-sol"');
    expect(commands).toContain('export APITOKEN_API_KEY="sk-pool-test-secret"');
    expect(commands).toContain("codex --profile apitoken");
  });

  it("keeps the key out of the TOML profile and uses a placeholder without a live key", () => {
    const commands = buildCodexCommands();

    expect(commands).toContain("YOUR_SK_POOL_API_KEY");
    // The profile references the environment variable by name only — the secret never enters TOML.
    expect(commands).not.toContain("APITOKEN_API_KEY = ");
  });

  it("writes the same profile on Windows without zsh syntax", () => {
    const powershell = buildCodexCommands("sk-pool-test-secret", "powershell");
    expect(powershell).toContain("Set-Content");
    expect(powershell).toContain('$env:APITOKEN_API_KEY = "sk-pool-test-secret"');

    const cmd = buildCodexCommands("sk-pool-test-secret", "cmd");
    expect(cmd).toContain('echo base_url = "https://openai.api.apitoken.sale/v1"');
    expect(cmd).toContain("set APITOKEN_API_KEY=sk-pool-test-secret");

    for (const commands of [powershell, cmd]) {
      expect(commands).toContain('wire_api = "responses"');
      expect(commands.trim().endsWith("codex --profile apitoken")).toBe(true);
      expect(commands).not.toContain("~/.zshrc");
      expect(commands).not.toContain("<< 'EOF'");
    }
  });
});

describe("agent handoff brief", () => {
  it("includes every value an AI agent needs for both API surfaces and the newly issued key", () => {
    const handoff = buildAgentHandoff({
      apiKey: "sk-pool-test-secret",
      docsUrl: "/docs",
      language: "en",
    });

    expect(handoff).toContain(CLAUDE_API_BASE_URL);
    expect(handoff).toContain(CLAUDE_MESSAGES_URL);
    expect(handoff).toContain(OPENAI_API_BASE_URL);
    expect(handoff).toContain(OPENAI_RESPONSES_URL);
    expect(handoff).toContain("Authorization: Bearer sk-pool-test-secret");
    expect(handoff).toContain("sk-pool-test-secret");
    expect(handoff).toContain("ANTHROPIC_API_KEY");
    expect(handoff).toContain("x-api-key");
    expect(handoff).toContain("https://apitoken.sale/docs");
    expect(handoff).toContain("never commit it");
  });

  it("uses a clear placeholder when the one-time key is unavailable", () => {
    const handoff = buildAgentHandoff({ apiKey: null, docsUrl: "/docs", language: "ru" });
    expect(handoff).toContain("YOUR_SK_POOL_API_KEY");
    expect(handoff).toContain("Подключи этот проект");
    expect(handoff).toContain(OPENAI_API_BASE_URL);
  });
});
