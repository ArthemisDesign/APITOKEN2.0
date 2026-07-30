import { NextResponse } from "next/server";
import { loadAdminKeyLookup } from "@/lib/keys";
import { internalAdminActor } from "@/lib/internal-admin";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * Карта engine-аккаунт → метка/номинал/продавец/профиль для единой админки.
 * Метаданные без live-балансов и без секретов; доступ — только через Caddy
 * с server-side control credential, как у остальных internal-admin роутов.
 */
export async function GET(request: Request): Promise<NextResponse> {
  if (!internalAdminActor(request)) return NextResponse.json({ error: "not_found" }, { status: 404 });
  const payload = await loadAdminKeyLookup();
  return NextResponse.json(payload, { headers: { "cache-control": "no-store" } });
}
