import { describe, expect, it } from "vitest";
import { canPublishExternally, slugify } from "./gate";

describe("blog-first publication gate", () => {
  it("stays locked until the canonical blog post has a publication timestamp", () => {
    expect(canPublishExternally(null)).toBe(false);
    expect(canPublishExternally({ blog_post: { slug: "post", status: "draft", published_at: null, locale: "en" } })).toBe(false);
    expect(canPublishExternally({ blog_post: { slug: "post", status: "published", published_at: null, locale: "en" } })).toBe(false);
    expect(canPublishExternally({ blog_post: { slug: "post", status: "published", published_at: "2026-07-20", locale: "en" } })).toBe(true);
  });

  it("creates stable English SEO slugs", () => {
    expect(slugify("Claude Sonnet 5: API Test!")).toBe("claude-sonnet-5-api-test");
  });
});
