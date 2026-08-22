import { buildApiReferenceMarkdown } from "@/lib/md-pages";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-dynamic";

export function GET(): Response {
  return new Response(buildApiReferenceMarkdown(), {
    status: 200,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "x-markdown-source": `${SITE_ORIGIN}/docs`,
      "cache-control": "no-store",
    },
  });
}
