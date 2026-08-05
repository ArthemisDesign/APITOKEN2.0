import { NextResponse } from "next/server";
import {
  applySellerKeyAction,
  listAdminSellers,
  sellerExists,
  SELLER_ACTIONS,
  type SellerAction,
} from "@/lib/admin-sellers";
import { internalAdminActor } from "@/lib/internal-admin";
import { readJsonLimited } from "@/lib/request-guard";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const NO_STORE = { "cache-control": "no-store" };

function json(body: unknown, status = 200): NextResponse {
  return NextResponse.json(body, { status, headers: NO_STORE });
}

/** Публичный вызов не должен даже узнать, что такой путь есть. */
function forbidden(): NextResponse {
  return json({ error: "not_found" }, 404);
}

function isSellerName(value: unknown): value is string {
  return typeof value === "string"
    && value.length > 0
    && value.length <= 128
    && !/[\u0000-\u001f\u007f]/.test(value);
}

export async function GET(request: Request): Promise<NextResponse> {
  if (!internalAdminActor(request)) return forbidden();
  return json({ sellers: await listAdminSellers() });
}

export async function POST(request: Request): Promise<NextResponse> {
  const actor = internalAdminActor(request);
  if (!actor) return forbidden();

  let body: { createdBy?: unknown; action?: unknown };
  try {
    body = await readJsonLimited<typeof body>(request);
  } catch {
    return json({ error: "invalid_body" }, 400);
  }

  const createdBy = body.createdBy;
  const action = body.action;
  if (!isSellerName(createdBy) || !SELLER_ACTIONS.includes(action as SellerAction)) {
    return json({ error: "invalid_body" }, 400);
  }
  // Издателя без единой партии не существует: иначе опечатка в имени молча
  // отвечала бы «0 ключей» и выглядела как успешное аннулирование.
  if (!(await sellerExists(createdBy))) return json({ error: "unknown_seller" }, 404);

  try {
    return json(await applySellerKeyAction({
      createdBy,
      action: action as SellerAction,
      actor,
    }));
  } catch {
    return json({ error: "seller_action_failed" }, 502);
  }
}
