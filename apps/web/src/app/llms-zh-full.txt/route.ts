import { buildLlmsFull } from "@/lib/llms";

export const dynamic = "force-dynamic";

export function GET(): Response {
  return new Response(buildLlmsFull("zh"), {
    status: 200,
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store" },
  });
}
