import { NextResponse } from "next/server";
import { internalAdminActor } from "@/lib/internal-admin";
import { guardRequest, readJsonLimited } from "@/lib/request-guard";
import { runOpenKeysStrictBackfill } from "@/lib/strict-backfill";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const NO_STORE = { "cache-control": "no-store" };
const ACCOUNT_ID = /^acct_[A-Za-z0-9_-]{1,195}$/;
const MAX_BATCH = 50;

function json(body: unknown, status = 200): NextResponse {
  return NextResponse.json(body, { status, headers: NO_STORE });
}

/**
 * POST /api/internal/admin/strict-backfill — bounded, idempotent backfill of pre-existing
 * OpenKeys engine accounts onto the direct strict path (release-v2 retirement, phase 2.2;
 * runbook docs/ops/PRICING_RELEASE_BACKFILL.md). Body: `{ "limit"?: 1..50 (default 5),
 * "account_ids"?: ["acct_…", …] }` — the explicit list is the canary mode. Each account is
 * independent: the response carries the per-account outcome and nothing rolls back.
 */
export async function POST(request: Request): Promise<NextResponse> {
  if (!internalAdminActor(request)) return json({ error: "not_found" }, 404);
  const rejected = guardRequest(request, "admin-strict-backfill", 30, 60_000);
  if (rejected) return rejected;

  let body: { limit?: unknown; account_ids?: unknown };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return json({ error: "invalid_body" }, 400);
  }
  const limit = body.limit === undefined ? 5 : body.limit;
  if (
    typeof limit !== "number" || !Number.isSafeInteger(limit) || limit < 1 || limit > MAX_BATCH
    || (body.account_ids !== undefined && (
      !Array.isArray(body.account_ids)
      || body.account_ids.length > MAX_BATCH
      || body.account_ids.some((id) => typeof id !== "string" || !ACCOUNT_ID.test(id))
    ))
  ) {
    return json({ error: "invalid_body" }, 400);
  }

  try {
    const summary = await runOpenKeysStrictBackfill({
      limit,
      accountIds: body.account_ids as string[] | undefined,
    });
    return json(summary);
  } catch {
    return json({ error: "strict_backfill_unavailable" }, 502);
  }
}
