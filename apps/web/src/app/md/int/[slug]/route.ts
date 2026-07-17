import { buildIntegrationMarkdown, integrationSlugs } from "@/lib/md-pages";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

export function generateStaticParams() {
  return integrationSlugs.map((slug) => ({ slug }));
}

export async function GET(_request: Request, { params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const body = buildIntegrationMarkdown(slug);
  if (!body) return new Response("Not found\n", { status: 404, headers: { "content-type": "text/plain; charset=utf-8" } });
  return new Response(body, {
    status: 200,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "x-markdown-source": `${SITE_ORIGIN}/int-${slug}`,
      "cache-control": "public, max-age=3600",
    },
  });
}
