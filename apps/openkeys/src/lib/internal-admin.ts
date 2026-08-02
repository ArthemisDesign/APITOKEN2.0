import "server-only";
import { createHash, timingSafeEqual } from "node:crypto";
import { loadConfig } from "./config";

function credentialDigest(value: string): Buffer {
  return createHash("sha256").update(value).digest();
}

/** Loopback/internal read contracts use the server credential without browser actor identity. */
export function internalControlCredential(
  request: Request,
  expectedCredential = loadConfig().engineControlKey,
): boolean {
  const supplied = request.headers.get("x-openkeys-control-key") ?? "";
  return Boolean(supplied)
    && supplied.length <= 4_096
    && timingSafeEqual(credentialDigest(supplied), credentialDigest(expectedCredential));
}

/**
 * Caddy очищает клиентскую identity, проверяет managed-admin и только затем
 * инжектит actor + server-side control credential в закрытый upstream.
 */
export function internalAdminActor(
  request: Request,
  expectedCredential = loadConfig().engineControlKey,
): string | null {
  const actor = request.headers.get("x-admin-actor")?.trim() ?? "";
  if (!actor || actor.length > 128 || /[\u0000-\u001f\u007f]/.test(actor)) {
    return null;
  }
  return internalControlCredential(request, expectedCredential) ? actor : null;
}
