import { NextResponse } from "next/server";
import {
  loadPayingKeys,
  type AdminKeyStatusFilter,
  type PayingKeysDays,
} from "@/lib/keys";
import { internalAdminActor } from "@/lib/internal-admin";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const NO_STORE = { "cache-control": "no-store" };

function json(body: unknown, status = 200): NextResponse {
  return NextResponse.json(body, { status, headers: NO_STORE });
}

function integer(value: string | null, fallback: number, min: number, max: number): number | null {
  if (value === null || value === "") return fallback;
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max ? parsed : null;
}

export async function GET(request: Request): Promise<NextResponse> {
  if (!internalAdminActor(request)) return json({ error: "not_found" }, 404);

  const params = new URL(request.url).searchParams;
  const rawDays = params.get("days") ?? "30";
  const days = (["1", "7", "30"] as const).includes(rawDays as `${PayingKeysDays}`)
    ? Number(rawDays) as PayingKeysDays
    : null;
  const limit = integer(params.get("limit"), 50, 1, 100);
  const offset = integer(params.get("offset"), 0, 0, 100_000);
  const q = (params.get("q") ?? "").trim();
  const rawStatus = params.get("status") ?? "all";
  const status = (["all", "active", "disabled"] as const).includes(rawStatus as AdminKeyStatusFilter)
    ? rawStatus as AdminKeyStatusFilter
    : null;
  if (days === null || limit === null || offset === null || q.length > 80 || status === null) {
    return json({ error: "invalid_query" }, 400);
  }

  return json(await loadPayingKeys({ days, limit, offset, q, status }));
}
