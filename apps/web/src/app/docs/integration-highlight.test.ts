import { describe, expect, it } from "vitest";
import { detectHighlightLang, highlightCode } from "./integration-highlight";

function classesFor(code: string, text: string): Array<string | null> {
  return highlightCode(code).filter((token) => token.text === text).map((token) => token.cls);
}

describe("detectHighlightLang", () => {
  it("detects JSON, TOML and shell snippets", () => {
    expect(detectHighlightLang('{\n  "provider": {}\n}')).toBe("json");
    expect(detectHighlightLang('model = "gpt-5.6-sol"\nmodel_provider = "apitoken"')).toBe("toml");
    expect(detectHighlightLang('export ANTHROPIC_BASE_URL="https://api.apitoken.sale"')).toBe("shell");
    expect(detectHighlightLang('$env:APITOKEN_API_KEY = "sk-pool-•••"')).toBe("shell");
  });
});

describe("highlightCode", () => {
  it("marks shell commands, variables, strings and comments", () => {
    const code = '# connect\nexport ANTHROPIC_API_KEY="sk-pool-•••"\nunset ANTHROPIC_AUTH_TOKEN';
    expect(classesFor(code, "export")).toContain("k");
    expect(classesFor(code, "unset")).toContain("k");
    expect(classesFor(code, '"sk-pool-•••"')).toContain("s");
    expect(classesFor(code, "ANTHROPIC_API_KEY")).toContain("v");
    expect(classesFor(code, "ANTHROPIC_AUTH_TOKEN")).toContain("v");
    expect(highlightCode(code)[0]).toMatchObject({ text: "# connect", cls: "c" });
  });

  it("marks PowerShell variables and cmd set command", () => {
    expect(classesFor("$env:APITOKEN_API_KEY = \"x\"", "$env:APITOKEN_API_KEY")).toContain("v");
    expect(classesFor('set "APITOKEN_API_KEY=x"', "set")).toContain("k");
    expect(classesFor("Remove-Item Env:ANTHROPIC_AUTH_TOKEN -ErrorAction SilentlyContinue", "Env:ANTHROPIC_AUTH_TOKEN")).toContain("v");
  });

  it("marks JSON keys, string values and literals", () => {
    const code = '{\n  "baseURL": "https://api.apitoken.sale",\n  "reasoning": true\n}';
    expect(classesFor(code, '"baseURL"')).toContain("v");
    expect(classesFor(code, '"https://api.apitoken.sale"')).toContain("s");
    expect(classesFor(code, "true")).toContain("k");
  });

  it("marks TOML sections, keys and string values", () => {
    const code = 'model = "gpt-5.6-sol"\n\n[model_providers.apitoken]\nwire_api = "responses"';
    expect(classesFor(code, "[model_providers.apitoken]")).toContain("k");
    expect(classesFor(code, "wire_api ")).toContain("v");
    expect(classesFor(code, '"responses"')).toContain("s");
  });

  it("round-trips the source text exactly", () => {
    const samples = [
      'export A="b"\n\nclaude --model claude-opus-4-8',
      '{\n  "a": [1, true, null]\n}',
      'model = "x"\n[model_providers.apitoken]\nbase_url = "https://openai.api.apitoken.sale/v1"',
      '/status\n\nReply with exactly: connected',
      'hermes model\n\nProvider: Custom endpoint (self-hosted / VLLM / etc.)',
    ];
    for (const sample of samples) {
      expect(highlightCode(sample).map((token) => token.text).join("")).toBe(sample);
    }
  });
});
