import type { MetadataRoute } from "next";
import { absoluteUrl, LAST_CONTENT_UPDATE, sitemapPages } from "@/lib/seo";

export default function sitemap(): MetadataRoute.Sitemap {
  return sitemapPages.map((page) => ({
    url: absoluteUrl(page.path),
    lastModified: LAST_CONTENT_UPDATE,
    changeFrequency: page.changeFrequency,
    priority: page.priority,
  }));
}
