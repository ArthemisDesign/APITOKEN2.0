import "server-only";
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { cookies } from "next/headers";
import { loadConfig, type OpenkeysConfig } from "./config";

export const SESSION_COOKIE = "openkeys_admin";

function sign(config: OpenkeysConfig, payload: string): string {
  return createHmac("sha256", config.sessionSecret).update(payload).digest("base64url");
}

function digest(value: string): string {
  return createHmac("sha256", "openkeys-credential").update(value).digest("base64");
}

function constantTimeEquals(a: string, b: string): boolean {
  const left = Buffer.from(a);
  const right = Buffer.from(b);
  if (left.length !== right.length) return false;
  return timingSafeEqual(left, right);
}

/**
 * Возвращает имя учётки при совпадении пары. Сверяем ВСЕ учётки без раннего выхода,
 * чтобы время ответа не зависело ни от существования логина, ни от его позиции в списке.
 */
export function authenticate(user: string, password: string, config = loadConfig()): string | null {
  let matched: string | null = null;
  for (const account of config.adminAccounts) {
    const userOk = constantTimeEquals(digest(user), digest(account.user));
    const passwordOk = constantTimeEquals(digest(password), digest(account.password));
    if (userOk && passwordOk) matched = account.user;
  }
  return matched;
}

export function issueSessionValue(user: string, config = loadConfig()): { value: string; maxAge: number } {
  const expiresAt = Date.now() + config.sessionTtlSeconds * 1000;
  const nonce = randomBytes(9).toString("base64url");
  const encodedUser = Buffer.from(user, "utf8").toString("base64url");
  const signature = sign(config, `${encodedUser}.${expiresAt}.${nonce}`);
  return { value: `${expiresAt}.${nonce}.${encodedUser}.${signature}`, maxAge: config.sessionTtlSeconds };
}

/** Имя админа из подписанной куки, либо null. Отозванная учётка перестаёт проходить сразу. */
export function sessionUser(value: string | undefined, config = loadConfig()): string | null {
  if (!value) return null;
  const parts = value.split(".");
  if (parts.length !== 4) return null;

  const [expiresRaw, nonce, encodedUser, signature] = parts as [string, string, string, string];
  if (!/^\d+$/.test(expiresRaw)) return null;
  if (Number(expiresRaw) <= Date.now()) return null;
  if (!constantTimeEquals(signature, sign(config, `${encodedUser}.${expiresRaw}.${nonce}`))) return null;

  const user = Buffer.from(encodedUser, "base64url").toString("utf8");
  return config.adminAccounts.some((account) => account.user === user) ? user : null;
}

export async function currentAdmin(): Promise<string | null> {
  try {
    const store = await cookies();
    return sessionUser(store.get(SESSION_COOKIE)?.value);
  } catch {
    return null;
  }
}
