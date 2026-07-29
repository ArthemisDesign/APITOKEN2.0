import { NextResponse } from "next/server";
import { listBatches, listKeys, markKeyDelivered, removeAllStock, removeKey } from "@/lib/keys";
import { currentAdmin } from "@/lib/session";
import { guardRequest, readJsonLimited } from "@/lib/request-guard";
import { parseApiType } from "@/lib/api-product";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function unauthorized(): NextResponse {
  return NextResponse.json({ error: "unauthorized" }, { status: 401 });
}

export async function GET(): Promise<NextResponse> {
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  const [keys, batches] = await Promise.all([listKeys(admin), listBatches(admin)]);
  return NextResponse.json({ admin, keys, batches });
}

/** Отметка «выдан» или снятие со склада. Чужие ключи недоступны. */
export async function POST(request: Request): Promise<NextResponse> {
  const rejected = guardRequest(request, "admin-keys", 60, 60_000);
  if (rejected) return rejected;
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  let body: { id?: unknown; action?: unknown; apiType?: unknown };
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
      if (body.apiType !== undefined && !apiType) {
        return NextResponse.json({ error: "invalid_body" }, { status: 400 });
      }
      return NextResponse.json({ ok: true, removed: await removeAllStock(admin, apiType ?? undefined) });
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
