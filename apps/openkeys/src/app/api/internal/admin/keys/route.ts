import { NextResponse } from "next/server";
import {
  loadAdminKeyDirectory,
  setKeyEnabledForInternalAdmin,
  type AdminKeyStatusFilter,
} from "@/lib/keys";
import { parseAdminUsageFilter } from "@/lib/admin-directory";
import { internalAdminActor } from "@/lib/internal-admin";
import { readJsonLimited } from "@/lib/request-guard";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function forbidden(): NextResponse {
  return NextResponse.json({ error: "not_found" }, { status: 404 });
}

function integer(value: string | null, fallback: number, min: number, max: number): number | null {
  if (value === null || value === "") return fallback;
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max ? parsed : null;
}

export async function GET(request: Request): Promise<NextResponse> {
  if (!internalAdminActor(request)) return forbidden();
  const params = new URL(request.url).searchParams;
  const limit = integer(params.get("limit"), 50, 1, 100);
  const offset = integer(params.get("offset"), 0, 0, 100_000);
  const q = (params.get("q") ?? "").trim();
  const batchId = params.get("batch") || null;
  const rawStatus = params.get("status") ?? "all";
  const status = (["all", "active", "disabled"] as const).includes(rawStatus as AdminKeyStatusFilter)
    ? rawStatus as AdminKeyStatusFilter
    : null;
  const usage = parseAdminUsageFilter(params.get("usage") ?? "all");
  if (limit === null || offset === null || q.length > 80 || (batchId !== null && !UUID.test(batchId)) || !status || !usage) {
    return NextResponse.json({ error: "invalid_query" }, { status: 400 });
  }

  const payload = await loadAdminKeyDirectory({ limit, offset, q, batchId, status, usage });
  return NextResponse.json(payload, { headers: { "cache-control": "no-store" } });
}

export async function POST(request: Request): Promise<NextResponse> {
  if (!internalAdminActor(request)) return forbidden();
  let body: { id?: unknown; enabled?: unknown };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }
  if (typeof body.id !== "string" || !UUID.test(body.id) || typeof body.enabled !== "boolean") {
    return NextResponse.json({ error: "invalid_body" }, { status: 400 });
  }
  try {
    const updated = await setKeyEnabledForInternalAdmin(body.id, body.enabled);
    if (!updated) return NextResponse.json({ error: "not_found" }, { status: 404 });
    return NextResponse.json({ ok: true });
  } catch {
    return NextResponse.json({ error: "engine_status_update_failed" }, { status: 502 });
  }
}
