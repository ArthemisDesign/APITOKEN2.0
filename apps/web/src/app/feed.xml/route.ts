import { articleUpdatedDate, learnArticles, learnPath } from "@/lib/learn";
import { absoluteUrl, SITE_NAME } from "@/lib/seo";

// RSS 2.0 feed of the learn cluster (EN). Static output, rebuilt on deploy.
export const dynamic = "force-static";

function escapeXml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

export function GET(): Response {
  const items = [...learnArticles]
    .sort((a, b) => articleUpdatedDate(b.slug).getTime() - articleUpdatedDate(a.slug).getTime())
    .map((article) => {
      const url = absoluteUrl(learnPath(article.slug, "en"));
      const date = articleUpdatedDate(article.slug).toUTCString();
      return [
        "    <item>",
        `      <title>${escapeXml(article.title)}</title>`,
        `      <link>${escapeXml(url)}</link>`,
        `      <guid isPermaLink="true">${escapeXml(url)}</guid>`,
        `      <pubDate>${date}</pubDate>`,
        `      <description>${escapeXml(article.description)}</description>`,
        "    </item>",
      ].join("\n");
    })
    .join("\n");

  const lastBuild = new Date(
    Math.max(...learnArticles.map((article) => articleUpdatedDate(article.slug).getTime())),
  ).toUTCString();

  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>${escapeXml(`${SITE_NAME} — Claude API guides`)}</title>
    <link>${escapeXml(absoluteUrl("/docs/learn"))}</link>
    <atom:link href="${escapeXml(absoluteUrl("/feed.xml"))}" rel="self" type="application/rss+xml"/>
    <description>Practical guides for buying, setting up and getting the most from the Claude API with apiToken.sale.</description>
    <language>en</language>
    <lastBuildDate>${lastBuild}</lastBuildDate>
${items}
  </channel>
</rss>
`;

  return new Response(xml, {
    headers: { "content-type": "application/rss+xml; charset=utf-8" },
  });
}
