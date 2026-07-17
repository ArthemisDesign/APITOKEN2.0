import { buildModelsMarkdown } from "@/lib/md-pages";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(buildModelsMarkdown(), {
    status: 200,
    headers: {
      "content-type": "text/markdown; charset=utf-8",
      "x-markdown-source": `${SITE_ORIGIN}/models`,
      "cache-control": "public, max-age=3600",
    },
  });
}
