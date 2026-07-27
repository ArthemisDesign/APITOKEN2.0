import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { SESSION_COOKIE, credentialsValid, issueSessionValue } from "@/lib/session";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(request: Request): Promise<NextResponse> {
  let body: { user?: unknown; password?: unknown };
  try {
    body = (await request.json()) as typeof body;
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const user = typeof body.user === "string" ? body.user : "";
  const password = typeof body.password === "string" ? body.password : "";
  if (!user || !password) return NextResponse.json({ error: "invalid_credentials" }, { status: 401 });

  const config = loadConfig();
  if (!credentialsValid(user, password, config)) {
    return NextResponse.json({ error: "invalid_credentials" }, { status: 401 });
  }

  const session = issueSessionValue(config);
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
