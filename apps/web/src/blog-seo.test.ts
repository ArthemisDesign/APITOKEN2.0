import { describe, expect, it } from "vitest";
import { buildFeed } from "./app/feed.xml/route";
import { buildSitemap } from "./app/sitemap";
import type { PublicBlogPostSummary } from "./lib/blog";
import { absoluteUrl } from "./lib/seo";

const post: PublicBlogPostSummary = {
  id: "post-id", slug: "verified-model-update", locale: "en", title: "Verified model update",
  excerpt: "What changed and what developers should do.", author_name: "apiToken.sale Editorial",
  seo_title: "Verified model update", seo_description: "Verified model update for AI API developers.",
  source_urls: ["https://example.com/source"], related_paths: ["/models"],
  published_at: "2026-07-20T10:00:00.000Z", updated_at: "2026-07-20T11:00:00.000Z",
};

describe("dynamic blog SEO streams", () => {
  it("adds every published canonical article to the sitemap", () => {
    const entry = buildSitemap([post]).find((candidate) => candidate.url === absoluteUrl("/blog/verified-model-update"));
    expect(entry).toMatchObject({ priority: 0.8, changeFrequency: "monthly" });
    expect(entry?.lastModified).toEqual(new Date(post.updated_at));
  });

  it("adds published canonical articles to RSS with escaped editorial copy", () => {
    const feed = buildFeed([{ ...post, title: "Model & API <update>" }]);
    expect(feed).toContain("Model &amp; API &lt;update&gt;");
    expect(feed).toContain(absoluteUrl("/blog/verified-model-update"));
    expect(feed).toContain(new Date(post.published_at).toUTCString());
  });
});
