import type { MetadataRoute } from "next";
import { articlesForLocale, articleUpdatedDate, learnHubPath, learnPath, LOCALES } from "@/lib/learn";
import { claudeModels, modelPath } from "@/lib/models";
import { absoluteUrl, LAST_CONTENT_UPDATE, sitemapPages } from "@/lib/seo";
import { blogPath, listBlogPosts, type PublicBlogPostSummary } from "@/lib/blog";

const MODELS_LAUNCH = new Date("2026-07-17T00:00:00.000Z");

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  return buildSitemap(await listBlogPosts());
}

export function buildSitemap(blogPosts: PublicBlogPostSummary[] = []): MetadataRoute.Sitemap {
  const corePages: MetadataRoute.Sitemap = sitemapPages.map((page) => ({
    url: absoluteUrl(page.path),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: page.changeFrequency,
    priority: page.priority,
  }));

  const infoPages: MetadataRoute.Sitemap = [
    { url: absoluteUrl("/about"), changeFrequency: "monthly" as const, priority: 0.6 },
    { url: absoluteUrl("/blog"), changeFrequency: "daily" as const, priority: 0.8 },
    { url: absoluteUrl("/contacts"), changeFrequency: "monthly" as const, priority: 0.5 },
    { url: absoluteUrl("/changelog"), changeFrequency: "weekly" as const, priority: 0.5 },
    { url: absoluteUrl("/status"), changeFrequency: "weekly" as const, priority: 0.4 },
  ].map((page) => ({ ...page, lastModified: LAST_CONTENT_UPDATE }));

  // Russian mirrors of the marketing pages (everything except /docs, which is
  // not mirrored). Home "/" maps to "/ru".
  const ruCorePages: MetadataRoute.Sitemap = sitemapPages
    .filter((page) => page.path !== "/docs")
    .map((page) => ({
      url: absoluteUrl(page.path === "/" ? "/ru" : `/ru${page.path}`),
      lastModified: LAST_CONTENT_UPDATE,
      changeFrequency: page.changeFrequency,
      priority: page.priority,
    }));

  const learnHubs: MetadataRoute.Sitemap = LOCALES.map((locale) => ({
    url: absoluteUrl(learnHubPath(locale)),
    lastModified: new Date(Math.max(...articlesForLocale(locale).map((slug) => articleUpdatedDate(slug).getTime()))),
    changeFrequency: "weekly",
    priority: 0.8,
  }));

  const learnPages: MetadataRoute.Sitemap = LOCALES.flatMap((locale) =>
    articlesForLocale(locale).map((slug) => ({
      url: absoluteUrl(learnPath(slug, locale)),
      lastModified: articleUpdatedDate(slug),
      changeFrequency: "monthly" as const,
      priority: 0.7,
    })),
  );

  const toolPages: MetadataRoute.Sitemap = [
    { url: absoluteUrl("/tools/claude-api-cost-calculator"), lastModified: MODELS_LAUNCH, changeFrequency: "monthly" as const, priority: 0.8 },
  ];

  // The /models hub itself ships via sitemapPages; only the per-model pages are added here.
  const modelPages: MetadataRoute.Sitemap = [
    ...claudeModels.map((model) => ({
      url: absoluteUrl(modelPath(model.slug)),
      lastModified: MODELS_LAUNCH,
      changeFrequency: "monthly" as const,
      priority: 0.7,
    })),
  ];

  const blogPages: MetadataRoute.Sitemap = blogPosts.map((post) => ({
    url: absoluteUrl(blogPath(post)),
    lastModified: new Date(post.updated_at),
    changeFrequency: "monthly" as const,
    priority: 0.8,
  }));

  return [...corePages, ...ruCorePages, ...infoPages, ...toolPages, ...modelPages, ...learnHubs, ...learnPages, ...blogPages];
}
