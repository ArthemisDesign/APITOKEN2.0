import { buildRobotsTxt } from "@/lib/robots";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(buildRobotsTxt(), {
    status: 200,
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "public, max-age=3600" },
  });
}
