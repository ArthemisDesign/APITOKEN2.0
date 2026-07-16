import { learnArticles, learnArticlesBySlug, renderLearnMarkdown } from "@/lib/learn";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

export function generateStaticParams() {
  return learnArticles.map((article) => ({ slug: article.slug }));
}

export async function GET(_request: Request, { params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const article = learnArticlesBySlug[slug];
  if (!article) {
    return new Response("Not found\n", { status: 404, headers: { "content-type": "text/plain; charset=utf-8" } });
  }
  const markdown = renderLearnMarkdown(article, SITE_ORIGIN);
  return new Response(markdown, {
    status: 200,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "x-markdown-source": `${SITE_ORIGIN}/docs/learn/${slug}`,
      "cache-control": "public, max-age=3600",
    },
  });
}
