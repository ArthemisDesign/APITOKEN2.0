import type { MetadataRoute } from "next";
import { articlesForLocale, learnHubPath, learnPath, LOCALES } from "@/lib/learn";
import { absoluteUrl, LAST_CONTENT_UPDATE, sitemapPages } from "@/lib/seo";

export default function sitemap(): MetadataRoute.Sitemap {
  const corePages: MetadataRoute.Sitemap = sitemapPages.map((page) => ({
    url: absoluteUrl(page.path),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: page.changeFrequency,
    priority: page.priority,
  }));

  const aboutPage: MetadataRoute.Sitemap = [{
    url: absoluteUrl("/about"),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: "monthly",
    priority: 0.6,
  }];

  const learnHubs: MetadataRoute.Sitemap = LOCALES.map((locale) => ({
    url: absoluteUrl(learnHubPath(locale)),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: "weekly",
    priority: 0.8,
  }));

  const learnPages: MetadataRoute.Sitemap = LOCALES.flatMap((locale) =>
    articlesForLocale(locale).map((slug) => ({
      url: absoluteUrl(learnPath(slug, locale)),
      lastModified: LAST_CONTENT_UPDATE,
      changeFrequency: "monthly" as const,
      priority: 0.7,
    })),
  );

  return [...corePages, ...aboutPage, ...learnHubs, ...learnPages];
}
