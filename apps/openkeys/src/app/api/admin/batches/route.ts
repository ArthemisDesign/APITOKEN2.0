import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { BatchIssuanceError, MAX_BATCH_QUANTITY, issueBatch, listBatches } from "@/lib/keys";
import { formatUsd, usdStringToNano } from "@/lib/money";
import { currentAdmin } from "@/lib/session";
import { guardRequest, readJsonLimited } from "@/lib/request-guard";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function unauthorized(): NextResponse {
  return NextResponse.json({ error: "unauthorized" }, { status: 401 });
}

export async function GET(): Promise<NextResponse> {
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  return NextResponse.json({ batches: await listBatches(admin) });
}

export async function POST(request: Request): Promise<NextResponse> {
  const rejected = guardRequest(request, "admin-batches", 30, 60_000);
  if (rejected) return rejected;
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  let body: { faceValueUsd?: unknown; quantity?: unknown; multBp?: unknown; label?: unknown; note?: unknown };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const config = loadConfig();
  const quantity = Number(body.quantity ?? 1);
  const multBp = body.multBp === undefined || body.multBp === "" ? config.defaultMultBp : Number(body.multBp);

  try {
    const faceValueNano = usdStringToNano(String(body.faceValueUsd ?? "").trim());
    if (!Number.isInteger(quantity) || quantity < 1 || quantity > MAX_BATCH_QUANTITY) {
      throw new Error(`Количество ключей должно быть от 1 до ${MAX_BATCH_QUANTITY}`);
    }

    const result = await issueBatch({
      faceValueNano,
      quantity,
      multBp,
      label: typeof body.label === "string" && body.label.trim() !== "" && body.label.length <= 200 ? body.label.trim() : null,
      note: typeof body.note === "string" && body.note.trim() !== "" && body.note.length <= 2000 ? body.note.trim() : null,
      createdBy: admin,
    });

    return NextResponse.json({
      batchId: result.batchId,
      faceValue: formatUsd(faceValueNano, 0),
      multBp,
      keys: result.keys,
    });
  } catch (error) {
    if (error instanceof BatchIssuanceError) {
      return NextResponse.json(
        { error: error.message, issuedCount: error.issuedCount },
        { status: 502 },
      );
    }
    const message = error instanceof Error ? error.message : "Не удалось выпустить ключи";
    return NextResponse.json({ error: message }, { status: 400 });
  }
}
