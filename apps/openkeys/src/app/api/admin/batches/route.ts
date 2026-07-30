import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { BatchIssuanceError, MAX_BATCH_QUANTITY, issueBatch, listBatches } from "@/lib/keys";
import { formatUsd, usdStringToNano } from "@/lib/money";
import { currentAdmin } from "@/lib/session";
import { guardRequest, readJsonLimited } from "@/lib/request-guard";
import { parseApiType } from "@/lib/api-product";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function unauthorized(): NextResponse {
  return NextResponse.json({ error: "unauthorized" }, { status: 401 });
}

function pageInteger(value: string | null, fallback: number, min: number, max: number): number | null {
  if (value === null || value === "") return fallback;
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max ? parsed : null;
}

export async function GET(request: Request): Promise<NextResponse> {
  const admin = await currentAdmin();
  if (!admin) return unauthorized();
  const params = new URL(request.url).searchParams;
  const limit = pageInteger(params.get("limit"), 20, 1, 50);
  const offset = pageInteger(params.get("offset"), 0, 0, 100_000);
  const q = (params.get("q") ?? "").trim();
  if (limit === null || offset === null || q.length > 80) {
    return NextResponse.json({ error: "invalid_query" }, { status: 400 });
  }
  return NextResponse.json({ admin, ...(await listBatches(admin, { limit, offset, q })) });
}

export async function POST(request: Request): Promise<NextResponse> {
  const rejected = guardRequest(request, "admin-batches", 30, 60_000);
  if (rejected) return rejected;
  const admin = await currentAdmin();
  if (!admin) return unauthorized();

  let body: {
    faceValueUsd?: unknown;
    quantity?: unknown;
    multBp?: unknown;
    label?: unknown;
    note?: unknown;
    apiType?: unknown;
  };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }

  const config = loadConfig();
  const quantity = Number(body.quantity ?? 1);
  const multBp = body.multBp === undefined || body.multBp === "" ? config.defaultMultBp : Number(body.multBp);
  const apiType = body.apiType === undefined ? "anthropic" : parseApiType(body.apiType);

  try {
    if (!apiType) throw new Error("Тип API должен быть anthropic или openai");
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
      apiType,
      createdBy: admin,
    });

    return NextResponse.json({
      batchId: result.batchId,
      faceValue: formatUsd(faceValueNano, 0),
      multBp,
      apiType,
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
