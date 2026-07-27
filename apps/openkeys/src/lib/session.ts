import "server-only";
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { cookies } from "next/headers";
import { loadConfig, type OpenkeysConfig } from "./config";

export const SESSION_COOKIE = "openkeys_admin";

function sign(config: OpenkeysConfig, payload: string): string {
  return createHmac("sha256", config.sessionSecret).update(payload).digest("base64url");
}

function constantTimeEquals(a: string, b: string): boolean {
  const left = Buffer.from(a);
  const right = Buffer.from(b);
  if (left.length !== right.length) return false;
  return timingSafeEqual(left, right);
}

/** Логин/пароль сверяем в постоянном времени, чтобы не отдавать их посимвольно. */
export function credentialsValid(user: string, password: string, config = loadConfig()): boolean {
  const userOk = constantTimeEquals(hash(user), hash(config.adminUser));
  const passwordOk = constantTimeEquals(hash(password), hash(config.adminPassword));
  return userOk && passwordOk;
}

function hash(value: string): string {
  return createHmac("sha256", "openkeys-credential").update(value).digest("base64");
}

export function issueSessionValue(config = loadConfig()): { value: string; maxAge: number } {
  const expiresAt = Date.now() + config.sessionTtlSeconds * 1000;
  const nonce = randomBytes(9).toString("base64url");
  const payload = `${config.adminUser}.${expiresAt}.${nonce}`;
  return { value: `${expiresAt}.${nonce}.${sign(config, payload)}`, maxAge: config.sessionTtlSeconds };
}

export function sessionValueValid(value: string | undefined, config = loadConfig()): boolean {
  if (!value) return false;
  const parts = value.split(".");
  if (parts.length !== 3) return false;

  const [expiresRaw, nonce, signature] = parts as [string, string, string];
  if (!/^\d+$/.test(expiresRaw)) return false;
  if (Number(expiresRaw) <= Date.now()) return false;

  return constantTimeEquals(signature, sign(config, `${config.adminUser}.${expiresRaw}.${nonce}`));
}

export async function isAdminAuthenticated(): Promise<boolean> {
  try {
    const store = await cookies();
    return sessionValueValid(store.get(SESSION_COOKIE)?.value);
  } catch {
    return false;
  }
}
