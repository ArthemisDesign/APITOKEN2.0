import { NextResponse } from "next/server";
import { loadKeyMonitor, setKeyEnabled } from "@/lib/keys";
import { currentAdmin } from "@/lib/session";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function unauthorized(): NextResponse {
  return NextResponse.json({ error: "unauthorized" }, { status: 401 });
}

export async function GET(): Promise<NextResponse> {
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  return NextResponse.json({ rows: await loadKeyMonitor(admin) });
}

/** Включение и отключение ключа. Чужие ключи недоступны. */
export async function POST(request: Request): Promise<NextResponse> {
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  let body: { id?: unknown; enabled?: unknown };
  try {
    body = (await request.json()) as typeof body;
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const id = typeof body.id === "string" ? body.id : "";
  if (!id || typeof body.enabled !== "boolean") {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  try {
    const applied = await setKeyEnabled(id, admin, body.enabled);
    if (!applied) return NextResponse.json({ error: "not_found" }, { status: 404 });
    return NextResponse.json({ ok: true });
  } catch {
    return NextResponse.json({ error: "Движок не принял смену статуса ключа" }, { status: 502 });
  }
}
