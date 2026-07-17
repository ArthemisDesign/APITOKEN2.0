import { articlesForLocale, learnPath, renderLearnMarkdown, resolveArticle } from "@/lib/learn";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

export function generateStaticParams() {
  return articlesForLocale("ko").map((slug) => ({ slug }));
}

export async function GET(_request: Request, { params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const article = resolveArticle(slug, "ko");
  if (!article) {
    return new Response("Not found\n", { status: 404, headers: { "content-type": "text/plain; charset=utf-8" } });
  }
  return new Response(renderLearnMarkdown(article, SITE_ORIGIN), {
    status: 200,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "x-markdown-source": `${SITE_ORIGIN}${learnPath(slug, "ko")}`,
      "cache-control": "public, max-age=3600",
    },
  });
}
