import { describe, expect, it } from "vitest";
import { ContentSourceService, detectSourcePlatform, extractHtmlMetadata, normalizeSourceUrl, stripHtml } from "./content-source.service.js";

describe("content source extraction", () => {
  it.each([
    ["https://x.com/anthropic/status/1", "x"],
    ["https://www.reddit.com/r/LocalLLaMA/comments/abc/post", "reddit"],
    ["https://vc.ru/ai/123", "vc-ru"],
    ["https://dzen.ru/a/example", "dzen"],
    ["https://habr.com/ru/articles/123", "habr"],
    ["https://example.com/post", "web"],
  ])("classifies %s as %s", (value, platform) => {
    expect(detectSourcePlatform(new URL(value))).toBe(platform);
  });

  it("rejects non-http and credential-bearing source URLs", () => {
    expect(() => normalizeSourceUrl("file:///etc/passwd")).toThrow("public HTTP");
    expect(() => normalizeSourceUrl("https://user:secret@example.com/post")).toThrow("public HTTP");
  });

  it("blocks private-network pages before making a fetch request", async () => {
    await expect(new ContentSourceService().extract({ sourceUrl: "http://127.0.0.1/private", locale: "en" }))
      .rejects.toThrow("Private network sources are not allowed");
  });

  it("extracts attributed article metadata and readable text", () => {
    const result = extractHtmlMetadata(`
      <html><head>
        <meta property="og:title" content="Model &amp; API update">
        <meta name="author" content="AI Lab">
        <meta property="article:published_time" content="2026-07-20T10:00:00Z">
      </head><body><article><h1>Ignored duplicate</h1><p>First result.</p><script>bad()</script><p>Second result.</p></article></body></html>
    `);
    expect(result.title).toBe("Model & API update");
    expect(result.author).toBe("AI Lab");
    expect(result.content).toContain("First result.\nSecond result.");
    expect(result.content).not.toContain("bad()");
    expect(result.publishedAt?.toISOString()).toBe("2026-07-20T10:00:00.000Z");
  });

  it("strips embeds without copying executable or decorative markup", () => {
    expect(stripHtml("<blockquote><p>Hello &amp; welcome</p></blockquote><style>.x{}</style>"))
      .toBe("Hello & welcome");
  });
});
