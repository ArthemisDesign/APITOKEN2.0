import "server-only";
import { createHmac, timingSafeEqual } from "node:crypto";
import { loadConfig, type OpenkeysConfig } from "./config";

/** Кука входа покупателя: хранит подписанную ссылку на баланс, а не сам ключ. */
export const USAGE_SESSION_COOKIE = "__Host-openkeys_view";
export const USAGE_SESSION_MAX_AGE = 30 * 24 * 60 * 60;

const VIEW_TOKEN = /^[A-Za-z0-9_-]{22}$/;

export function validViewToken(value: string): boolean {
  return VIEW_TOKEN.test(value);
}

function signature(payload: string, config: OpenkeysConfig): string {
  return createHmac("sha256", config.sessionSecret).update(`usage:${payload}`).digest("base64url");
}

function equalSignature(left: string, right: string): boolean {
  const a = Buffer.from(left);
  const b = Buffer.from(right);
  return a.length === b.length && timingSafeEqual(a, b);
}

export function issueUsageSession(viewToken: string, config = loadConfig()): string {
  if (!validViewToken(viewToken)) throw new Error("invalid view token");
  const expiresAt = Date.now() + USAGE_SESSION_MAX_AGE * 1000;
  const payload = `${viewToken}.${expiresAt}`;
  return `${payload}.${signature(payload, config)}`;
}

export function usageSessionToken(value: string | undefined, config = loadConfig()): string | null {
  if (!value) return null;
  const [viewToken, expiresRaw, supplied, ...extra] = value.split(".");
  if (extra.length || !viewToken || !expiresRaw || !supplied || !validViewToken(viewToken)) return null;
  if (!/^\d+$/.test(expiresRaw) || Number(expiresRaw) <= Date.now()) return null;
  const payload = `${viewToken}.${expiresRaw}`;
  return equalSignature(supplied, signature(payload, config)) ? viewToken : null;
}
