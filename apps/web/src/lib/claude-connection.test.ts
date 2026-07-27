import { describe, expect, it } from "vitest";
import {
  buildClaudeCodeCommands,
  buildClaudeAgentHandoff,
  CLAUDE_API_BASE_URL,
  CLAUDE_MESSAGES_URL,
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

  it("includes every value an AI agent needs and the newly issued key", () => {
    const handoff = buildClaudeAgentHandoff({
      apiKey: "sk-pool-test-secret",
      docsUrl: "/docs",
      language: "en",
    });

    expect(handoff).toContain(CLAUDE_API_BASE_URL);
    expect(handoff).toContain(CLAUDE_MESSAGES_URL);
    expect(handoff).toContain("sk-pool-test-secret");
    expect(handoff).toContain("ANTHROPIC_API_KEY");
    expect(handoff).toContain("x-api-key");
    expect(handoff).toContain("https://apitoken.sale/docs");
    expect(handoff).toContain("never commit it");
  });

  it("uses a clear placeholder when the one-time key is unavailable", () => {
    const handoff = buildClaudeAgentHandoff({ apiKey: null, docsUrl: "/docs", language: "ru" });
    expect(handoff).toContain("YOUR_SK_POOL_API_KEY");
    expect(handoff).toContain("Подключи этот проект");
  });
});
