export function GET(): Response {
  return Response.json({ ok: true, service: "content-studio" }, { headers: { "cache-control": "no-store" } });
}
