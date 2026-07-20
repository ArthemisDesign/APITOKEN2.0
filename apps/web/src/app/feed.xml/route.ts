import { articleUpdatedDate, learnArticles, learnPath } from "@/lib/learn";
import { absoluteUrl, SITE_NAME } from "@/lib/seo";
import { blogPath, listBlogPosts, type PublicBlogPostSummary } from "@/lib/blog";

export const revalidate = 60;

function escapeXml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

export async function GET(): Promise<Response> {
  return new Response(buildFeed(await listBlogPosts()), {
    headers: { "content-type": "application/rss+xml; charset=utf-8" },
  });
}

export function buildFeed(blogPosts: PublicBlogPostSummary[] = []): string {
  const guideItems = learnArticles.map((article) => {
      const url = absoluteUrl(learnPath(article.slug, "en"));
      const date = articleUpdatedDate(article.slug);
      return { date, xml: [
        "    <item>",
        `      <title>${escapeXml(article.title)}</title>`,
        `      <link>${escapeXml(url)}</link>`,
        `      <guid isPermaLink="true">${escapeXml(url)}</guid>`,
        `      <pubDate>${date.toUTCString()}</pubDate>`,
        `      <description>${escapeXml(article.description)}</description>`,
        "    </item>",
      ].join("\n") };
    });

  const dynamicItems = blogPosts.map((post) => ({
    date: new Date(post.published_at),
    xml: [
      "    <item>",
      `      <title>${escapeXml(post.title)}</title>`,
      `      <link>${escapeXml(absoluteUrl(blogPath(post)))}</link>`,
      `      <guid isPermaLink="true">${escapeXml(absoluteUrl(blogPath(post)))}</guid>`,
      `      <pubDate>${new Date(post.published_at).toUTCString()}</pubDate>`,
      `      <description>${escapeXml(post.excerpt)}</description>`,
      "    </item>",
    ].join("\n"),
  }));
  const items = [...guideItems, ...dynamicItems].sort((a, b) => b.date.getTime() - a.date.getTime())
    .map((item) => item.xml).join("\n");

  const lastBuild = new Date(
    Math.max(
      ...learnArticles.map((article) => articleUpdatedDate(article.slug).getTime()),
      ...blogPosts.map((post) => new Date(post.updated_at).getTime()),
    ),
  ).toUTCString();

  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>${escapeXml(`${SITE_NAME} — AI API field notes and guides`)}</title>
    <link>${escapeXml(absoluteUrl("/blog"))}</link>
    <atom:link href="${escapeXml(absoluteUrl("/feed.xml"))}" rel="self" type="application/rss+xml"/>
    <description>Verified AI API analysis and practical guides from apiToken.sale.</description>
    <language>en</language>
    <lastBuildDate>${lastBuild}</lastBuildDate>
${items}
  </channel>
</rss>
`;

  return xml;
}
