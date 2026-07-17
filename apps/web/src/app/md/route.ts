import { buildMdIndexMarkdown } from "@/lib/md-pages";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(buildMdIndexMarkdown(), {
    status: 200,
    headers: { "content-type": "text/markdown; charset=utf-8", "cache-control": "public, max-age=3600" },
  });
}
