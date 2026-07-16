import { describe, expect, it } from "vitest";
import { detectAiSource } from "./ai-source";

describe("detectAiSource", () => {
  it("detects major AI assistants from the referrer", () => {
    expect(detectAiSource("https://chatgpt.com/")).toBe("ChatGPT");
    expect(detectAiSource("https://www.perplexity.ai/search")).toBe("Perplexity");
    expect(detectAiSource("https://claude.ai/chat/123")).toBe("Claude");
    expect(detectAiSource("https://gemini.google.com/app")).toBe("Gemini");
    expect(detectAiSource("https://www.deepseek.com/")).toBe("DeepSeek");
  });

  it("detects AI sources from utm_source when referrer is empty", () => {
    expect(detectAiSource("", "perplexity")).toBe("Perplexity");
    expect(detectAiSource(null, "chatgpt")).toBe("ChatGPT");
  });

  it("returns null for regular search engines and direct visits", () => {
    expect(detectAiSource("https://www.google.com/search?q=claude+api")).toBeNull();
    expect(detectAiSource("https://duckduckgo.com/")).toBeNull();
    expect(detectAiSource("")).toBeNull();
    expect(detectAiSource(null)).toBeNull();
  });
});
