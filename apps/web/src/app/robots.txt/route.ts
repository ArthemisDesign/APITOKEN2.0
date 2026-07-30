import { buildRobotsTxt } from "@/lib/robots";

export const dynamic = "force-static";

// Превью-деплои (v2.apitoken.sale и ветки) — полный запрет обхода: staging-копия
// сайта не должна конкурировать с основным доменом в поиске. force-static ок:
// VERCEL_ENV известен на билде.
const isProductionDeployment = (process.env.VERCEL_ENV ?? "production") === "production";

export function GET(): Response {
  const body = isProductionDeployment ? buildRobotsTxt() : "User-agent: *\nDisallow: /\n";
  return new Response(body, {
    status: 200,
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "public, max-age=3600" },
  });
}
