import { API_BASE_URL } from "./api";

export interface PublicBlogPostSummary {
  id: string;
  slug: string;
  locale: "en" | "ru";
  title: string;
  excerpt: string;
  author_name: string;
  seo_title: string;
  seo_description: string;
  source_urls: string[];
  related_paths: string[];
  published_at: string;
  updated_at: string;
}

export interface PublicBlogPost extends PublicBlogPostSummary {
  body_markdown: string;
}

export async function listBlogPosts(): Promise<PublicBlogPostSummary[]> {
  try {
    const response = await fetch(`${API_BASE_URL}/blog/posts?limit=100`, { next: { revalidate: 60 } });
    if (!response.ok) return [];
    const payload = await response.json() as { posts?: PublicBlogPostSummary[] };
    return Array.isArray(payload.posts) ? payload.posts : [];
  } catch {
    return [];
  }
}

export async function getBlogPost(slug: string, locale: "en" | "ru" = "en"): Promise<PublicBlogPost | null> {
  for (const candidate of [locale, locale === "en" ? "ru" : "en"] as const) {
    try {
      const response = await fetch(`${API_BASE_URL}/blog/posts/${encodeURIComponent(slug)}?locale=${candidate}`, { next: { revalidate: 60 } });
      if (!response.ok) continue;
      const payload = await response.json() as { post?: PublicBlogPost };
      if (payload.post) return payload.post;
    } catch {
      // A temporary backend failure is rendered as not found and retried after revalidation.
    }
  }
  return null;
}

export function blogPath(post: Pick<PublicBlogPostSummary, "slug">): string {
  return `/blog/${post.slug}`;
}
