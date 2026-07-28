import { buildErrorsMarkdown } from "@/lib/md-pages";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(buildErrorsMarkdown(), {
    status: 200,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "x-markdown-source": `${SITE_ORIGIN}/docs/errors`,
      "cache-control": "public, max-age=3600",
    },
  });
}
