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
    expect(buildClaudeCodeCommands("sk-pool-test-secret")).toBe(`# Current terminal
export ANTHROPIC_BASE_URL="https://api.apitoken.sale"
export ANTHROPIC_API_KEY="sk-pool-test-secret"

# Future terminals
APITOKEN_ENV_FILE="\${XDG_CONFIG_HOME:-$HOME/.config}/apitoken/claude.env"
mkdir -p "\${APITOKEN_ENV_FILE%/*}"
(
  umask 077
  printf '%s\\n' 'export ANTHROPIC_BASE_URL="https://api.apitoken.sale"' 'export ANTHROPIC_API_KEY="sk-pool-test-secret"' > "$APITOKEN_ENV_FILE"
)
chmod 600 "$APITOKEN_ENV_FILE"

SHELL_PROFILE="\${ZDOTDIR:-$HOME}/.zshrc"
[ "\${SHELL##*/}" = "bash" ] && SHELL_PROFILE="$HOME/.bashrc"
touch "$SHELL_PROFILE"
SOURCE_LINE="[ -f \\"$APITOKEN_ENV_FILE\\" ] && . \\"$APITOKEN_ENV_FILE\\""
grep -qxF "$SOURCE_LINE" "$SHELL_PROFILE" || printf '\\n%s\\n' "$SOURCE_LINE" >> "$SHELL_PROFILE"

# Start Claude Code
claude`);
    expect(buildClaudeCodeCommands()).toContain("YOUR_SK_POOL_API_KEY");
  });

  it("persists Claude credentials for future zsh and bash terminals", () => {
    const commands = buildClaudeCodeCommands("sk-pool-test-secret");

    expect(commands).toContain('/apitoken/claude.env');
    expect(commands).toContain("umask 077");
    expect(commands).toContain('chmod 600 "$APITOKEN_ENV_FILE"');
    expect(commands).toContain('.zshrc');
    expect(commands).toContain('.bashrc');
    expect(commands).toContain('grep -qxF "$SOURCE_LINE"');
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
