import { NextResponse } from "next/server";
import { listKeys, markKeyDelivered, removeAllStock, removeKey } from "@/lib/keys";
import { currentAdmin } from "@/lib/session";
import { guardRequest, readJsonLimited } from "@/lib/request-guard";
import { parseApiType } from "@/lib/api-product";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function unauthorized(): NextResponse {
  return NextResponse.json({ error: "unauthorized" }, { status: 401 });
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export async function GET(request: Request): Promise<NextResponse> {
  const admin = await currentAdmin();
  if (!admin) return unauthorized();
  const batchId = new URL(request.url).searchParams.get("batchId") ?? "";
  if (!UUID.test(batchId)) return NextResponse.json({ error: "invalid_batch" }, { status: 400 });
  return NextResponse.json({ admin, keys: await listKeys(admin, batchId) });
}

/** Отметка «выдан» или снятие со склада. Чужие ключи недоступны. */
export async function POST(request: Request): Promise<NextResponse> {
  const rejected = guardRequest(request, "admin-keys", 60, 60_000);
  if (rejected) return rejected;
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  let body: { id?: unknown; action?: unknown; apiType?: unknown; batchId?: unknown };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const id = typeof body.id === "string" ? body.id : "";
  const action = body.action;

  try {
    if (action === "remove_all") {
      const apiType = body.apiType === undefined ? undefined : parseApiType(body.apiType);
      const batchId = body.batchId === undefined ? undefined : typeof body.batchId === "string" ? body.batchId : "";
      if ((body.apiType !== undefined && !apiType) || (batchId !== undefined && !UUID.test(batchId))) {
        return NextResponse.json({ error: "invalid_body" }, { status: 400 });
      }
      return NextResponse.json({ ok: true, removed: await removeAllStock(admin, apiType ?? undefined, batchId) });
    }
    if (!id || (action !== "deliver" && action !== "remove")) {
      return NextResponse.json({ error: "invalid_body" }, { status: 400 });
    }

    const applied = action === "deliver" ? await markKeyDelivered(id, admin) : await removeKey(id, admin);
    if (!applied) return NextResponse.json({ error: "not_found" }, { status: 404 });
    return NextResponse.json({ ok: true });
  } catch {
    return NextResponse.json({ error: "Не удалось изменить статус ключа" }, { status: 502 });
  }
}
