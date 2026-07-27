import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { SESSION_COOKIE, authenticate, issueSessionValue } from "@/lib/session";
import { guardRequest, readJsonLimited } from "@/lib/request-guard";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(request: Request): Promise<NextResponse> {
  const rejected = guardRequest(request, "admin-login", 10, 15 * 60_000);
  if (rejected) return rejected;
  let body: { user?: unknown; password?: unknown };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const user = typeof body.user === "string" && body.user.length <= 128 ? body.user : "";
  const password = typeof body.password === "string" && body.password.length <= 1024 ? body.password : "";
  if (!user || !password) return NextResponse.json({ error: "invalid_credentials" }, { status: 401 });

  const config = loadConfig();
  const authenticated = authenticate(user, password, config);
  if (!authenticated) {
    return NextResponse.json({ error: "invalid_credentials" }, { status: 401 });
  }

  const session = issueSessionValue(authenticated, config);
  const response = NextResponse.json({ ok: true });
  response.cookies.set(SESSION_COOKIE, session.value, {
    httpOnly: true,
    sameSite: "lax",
    secure: true,
    path: "/",
    maxAge: session.maxAge,
  });
  return response;
}
