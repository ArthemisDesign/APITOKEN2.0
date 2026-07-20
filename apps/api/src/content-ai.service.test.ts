import { ConfigService } from "@nestjs/config";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ContentAiService } from "./content-ai.service.js";
import type { Environment } from "./config.js";

const rules = {
  tone: "Direct", audience: "Developers", length: "500 words", linkPolicy: "One canonical link",
  requiredDisclosure: "", forbidden: ["invented facts"],
};

describe("content AI generation", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("guarantees an external draft carries a canonical URL placeholder", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      content: [{ type: "text", text: JSON.stringify({ title: "Result", excerpt: "Summary", bodyMarkdown: "Useful standalone analysis." }) }],
    }), { status: 200, headers: { "content-type": "application/json" } })));
    const service = createService();
    const draft = await service.generateDraft({
      brief: "Verified fact", sourceUrl: "https://example.com/source", profileKey: "reddit",
      profileName: "Reddit", rules, locale: "en",
    });
    expect(draft.bodyMarkdown).toContain("{{CANONICAL_URL}}");
  });

  it("does not add a canonical placeholder to the first-party blog draft", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      content: [{ type: "text", text: JSON.stringify({ title: "Result", excerpt: "Summary", bodyMarkdown: "# Analysis\n\nSources." }) }],
    }), { status: 200, headers: { "content-type": "application/json" } })));
    const draft = await createService().generateDraft({
      brief: "Verified fact", sourceUrl: "https://example.com/source", profileKey: "blog",
      profileName: "apiToken.sale blog", rules, locale: "en",
    });
    expect(draft.bodyMarkdown).not.toContain("{{CANONICAL_URL}}");
  });
});

function createService(): ContentAiService {
  return new ContentAiService(new ConfigService<Environment, true>({
    CONTENT_STUDIO_ENGINE_URL: "https://api.apitoken.sale",
    CONTENT_STUDIO_ENGINE_KEY: `sk-pool-${"x".repeat(32)}`,
    CONTENT_STUDIO_AI_MODEL: "claude-sonnet-5",
    CONTENT_STUDIO_AI_MAX_TOKENS: 1_000,
  } as Environment));
}
