import { NextResponse } from "next/server";
import { resolveViewTokenByApiKey } from "@/lib/keys";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/** Публичный: отдаёт ссылку на страницу расхода по самому ключу. Ключ не логируем. */
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

  return NextResponse.json({ viewToken });
}
