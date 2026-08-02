import { NextResponse } from "next/server";
import { internalControlCredential } from "@/lib/internal-admin";
import { loadOpenKeysPricingInventoryPageV2 } from "@/lib/pricing-inventory";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function limit(value: string | null): number | null {
  if (value === null || value === "") return 500;
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 1 && parsed <= 500 ? parsed : null;
}

export async function GET(request: Request): Promise<NextResponse> {
  if (!internalControlCredential(request)) {
    return NextResponse.json({ error: "not_found" }, { status: 404 });
  }
  const params = new URL(request.url).searchParams;
  const pageLimit = limit(params.get("limit"));
  const afterAccountId = params.get("after_account_id") || undefined;
  if (
    pageLimit === null
    || (afterAccountId !== undefined
      && (!afterAccountId.startsWith("acct_") || afterAccountId.length > 200))
  ) {
    return NextResponse.json({ error: "invalid_query" }, { status: 400 });
  }
  try {
    const inventory = await loadOpenKeysPricingInventoryPageV2({
      ...(afterAccountId === undefined ? {} : { afterAccountId }),
      limit: pageLimit,
    });
    return NextResponse.json({ inventory }, { headers: { "cache-control": "no-store" } });
  } catch {
    return NextResponse.json({ error: "inventory_unavailable" }, { status: 503 });
  }
}
