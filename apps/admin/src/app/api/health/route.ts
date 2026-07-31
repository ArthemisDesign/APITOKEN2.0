// Health-gate для watchdog: GET /api/health → 200 {"ok":true}.
export function GET() {
  return Response.json({ ok: true });
}
