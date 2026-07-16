import { describe, expect, it } from "vitest";
import manifest from "./app/manifest";
import robots from "./app/robots";
import sitemap from "./app/sitemap";
import {
  absoluteUrl,
  createNoIndexMetadata,
  createPageMetadata,
  integrationGuideSeo,
  seoPages,
  sitemapPages,
} from "./lib/seo";

describe("technical SEO", () => {
  it("publishes every canonical public route in the sitemap and excludes private flows", () => {
    const urls = sitemap().map((entry) => entry.url);
    const expected = sitemapPages.map((page) => absoluteUrl(page.path));

    expect(urls).toEqual(expected);
    expect(new Set(urls).size).toBe(urls.length);
    for (const privatePath of ["/login", "/register", "/dashboard", "/forgot-password", "/reset-password", "/verify-email", "/auth/callback"]) {
      expect(urls).not.toContain(absoluteUrl(privatePath));
    }
  });

  it("advertises the sitemap to crawlers and exposes a complete web manifest", () => {
    expect(robots().rules).toEqual([{ userAgent: "*", allow: "/" }]);
    expect(robots().sitemap).toBe(absoluteUrl("/sitemap.xml"));
    expect(manifest().start_url).toBe("/");
    expect(manifest().icons).toHaveLength(3);
  });

  it("gives each indexable page a unique description, canonical, and social URL", () => {
    const pages = [...Object.values(seoPages), ...Object.values(integrationGuideSeo)];
    expect(new Set(pages.map((page) => page.description)).size).toBe(pages.length);

    for (const page of pages) {
      const metadata = createPageMetadata(page);
      expect(metadata.alternates?.canonical).toBe(absoluteUrl(page.path));
      expect(metadata.openGraph?.url).toBe(absoluteUrl(page.path));
      expect(metadata.description).toBe(page.description);
    }
  });

  it("marks private utility pages noindex and nofollow", () => {
    const metadata = createNoIndexMetadata("Private", "Private account page");
    expect(metadata.robots).toMatchObject({ index: false, follow: false, nocache: true });
  });
});
