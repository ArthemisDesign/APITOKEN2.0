import { NextResponse } from "next/server";
import { SESSION_COOKIE } from "@/lib/session";
import { guardRequest } from "@/lib/request-guard";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(request: Request): Promise<NextResponse> {
  const rejected = guardRequest(request, "admin-logout", 30, 60_000);
  if (rejected) return rejected;
  const response = NextResponse.json({ ok: true });
  response.cookies.set(SESSION_COOKIE, "", {
    httpOnly: true,
    sameSite: "lax",
    secure: true,
    path: "/",
    maxAge: 0,
  });
  return response;
}
