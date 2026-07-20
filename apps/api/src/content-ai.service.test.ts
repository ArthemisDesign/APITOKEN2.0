import { ConfigService } from "@nestjs/config";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ContentAiService } from "./content-ai.service.js";
import type { Environment } from "./config.js";

const rules = {
  tone: "Direct", audience: "Developers", length: "500 words", linkPolicy: "One canonical link",
  requiredDisclosure: "", forbidden: ["invented facts"],
};

const briefInput = {
  sourceUrl: "https://x.com/example/status/1",
  title: "Example post",
  author: "Author",
  content: "A short source post.",
  references: [],
  locale: "en" as const,
};

describe("content AI generation", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("forces the exact brief schema through a tool response", async () => {
    const fetchMock = vi.fn(async (_url: unknown, init?: RequestInit) => {
      const request = JSON.parse(String(init?.body)) as {
        tools: Array<{ name: string; input_schema: { required: string[] } }>;
        tool_choice: { name: string };
      };
      expect(request.tools[0]).toMatchObject({ name: "return_content" });
      expect(request.tools[0]?.input_schema.required).toEqual(["briefMarkdown"]);
      expect(request.tool_choice.name).toBe("return_content");
      return messageResponse([{ type: "tool_use", name: "return_content", input: {
        briefMarkdown: "# Verified brief\n\nConfirmed fact.",
      } }]);
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(createService().generateBrief(briefInput))
      .resolves.toBe("# Verified brief\n\nConfirmed fact.");
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("accepts fenced JSON fallback and a legacy snake_case brief field", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => messageResponse([{ type: "text", text:
      "```json\n{\"brief_markdown\":\"# Verified brief\\n\\nFallback content.\"}\n```",
    }])));

    await expect(createService().generateBrief(briefInput))
      .resolves.toBe("# Verified brief\n\nFallback content.");
  });

  it("retries once after malformed JSON and accepts a corrected tool response", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(messageResponse([{ type: "text", text: "{\"briefMarkdown\":\"broken\nJSON\"}" }]))
      .mockResolvedValueOnce(messageResponse([{ type: "tool_use", name: "return_content", input: {
        briefMarkdown: "# Corrected brief",
      } }]));
    vi.stubGlobal("fetch", fetchMock);

    await expect(createService().generateBrief(briefInput)).resolves.toBe("# Corrected brief");
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const retry = JSON.parse(String((fetchMock.mock.calls[1]?.[1] as RequestInit | undefined)?.body)) as { system: string };
    expect(retry.system).toContain("previous response did not match");
  });

  it("fails clearly after one retry when the brief stays empty", async () => {
    const fetchMock = vi.fn(async () => messageResponse([{ type: "tool_use", name: "return_content", input: {
      briefMarkdown: "",
    } }]));
    vi.stubGlobal("fetch", fetchMock);

    await expect(createService().generateBrief(briefInput))
      .rejects.toThrow("AI response was not valid after an automatic retry");
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

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
      content: [{ type: "text", text: JSON.stringify({
        title: "Result", excerpt: "Summary",
        bodyMarkdown: "# Analysis\n\nUseful content.\n\nFull analysis: {{CANONICAL_URL}}\n\n## Sources\n\nOriginal source.",
      }) }],
    }), { status: 200, headers: { "content-type": "application/json" } })));
    const draft = await createService().generateDraft({
      brief: "Verified fact", sourceUrl: "https://example.com/source", profileKey: "blog",
      profileName: "apiToken.sale blog", rules, locale: "en",
    });
    expect(draft.bodyMarkdown).not.toContain("{{CANONICAL_URL}}");
    expect(draft.bodyMarkdown).toContain("Useful content.");
    expect(draft.bodyMarkdown).toContain("## Sources");
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

function messageResponse(content: Array<Record<string, unknown>>): Response {
  return new Response(JSON.stringify({ content, stop_reason: "tool_use" }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}
