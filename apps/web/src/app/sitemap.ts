import type { MetadataRoute } from "next";
import { learnArticles, learnPath, LEARN_HUB_PATH } from "@/lib/learn";
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

  const learnHub: MetadataRoute.Sitemap = [{
    url: absoluteUrl(LEARN_HUB_PATH),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: "weekly",
    priority: 0.8,
  }];

  const learnPages: MetadataRoute.Sitemap = learnArticles.map((article) => ({
    url: absoluteUrl(learnPath(article.slug)),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: "monthly",
    priority: 0.7,
  }));

  return [...corePages, ...aboutPage, ...learnHub, ...learnPages];
}
