import { NextResponse } from "next/server";
import { resolveViewTokenByApiKey } from "@/lib/keys";
import { USAGE_SESSION_COOKIE, USAGE_SESSION_MAX_AGE } from "@/lib/usage-session";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * Публичный вход по ключу. Ключ проверяется у движка и никуда не пишется:
 * в куку кладём только ссылку на баланс, поэтому утечка куки не отдаёт секрет.
 */
export async function POST(request: Request): Promise<NextResponse> {
  let body: { key?: unknown };
  try {
    body = (await request.json()) as typeof body;
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const key = typeof body.key === "string" ? body.key.trim() : "";
  if (!key) return NextResponse.json({ error: "not_found" }, { status: 404 });

  const viewToken = await resolveViewTokenByApiKey(key);
  if (!viewToken) return NextResponse.json({ error: "not_found" }, { status: 404 });

  const response = NextResponse.json({ viewToken });
  response.cookies.set(USAGE_SESSION_COOKIE, viewToken, {
    httpOnly: true,
    sameSite: "lax",
    secure: true,
    path: "/",
    maxAge: USAGE_SESSION_MAX_AGE,
  });
  return response;
}
