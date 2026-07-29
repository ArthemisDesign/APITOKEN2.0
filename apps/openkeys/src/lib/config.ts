import "server-only";

/**
 * Env читается ЛЕНИВО, внутри обработчиков. Next собирает страницы на билд-машине,
 * где прод-секретов нет — обращение к env на импорте уронило бы сборку.
 */
function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function optionalInt(name: string, fallback: number): number {
  const raw = process.env[name];
  if (raw === undefined || raw === "") return fallback;
  if (!/^\d+$/.test(raw)) throw new Error(`${name} must be a positive integer`);
  const value = Number(raw);
  if (!Number.isSafeInteger(value)) throw new Error(`${name} must be a safe integer`);
  return value;
}

function baseUrl(name: string, fallback: string, allowLoopbackHttp = false): string {
  const raw = process.env[name] ?? fallback;
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new Error(`${name} must be an absolute URL`);
  }
  const loopback = url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "::1";
  if (url.protocol !== "https:" && !(allowLoopbackHttp && loopback && url.protocol === "http:")) {
    throw new Error(`${name} must use HTTPS${allowLoopbackHttp ? " or loopback HTTP" : ""}`);
  }
  if (url.username || url.password || url.search || url.hash) throw new Error(`${name} must not contain credentials or parameters`);
  return url.origin;
}

export interface AdminAccount {
  user: string;
  password: string;
}

export interface OpenkeysConfig {
  databaseUrl: string;
  engineBaseUrl: string;
  engineControlKey: string;
  enginePublicBaseUrl: string;
  engineOpenAiPublicBaseUrl: string;
  adminAccounts: AdminAccount[];
  sessionSecret: string;
  sessionTtlSeconds: number;
  defaultMultBp: number;
  publicBaseUrl: string;
}

/**
 * Учётки админки. Основная пара живёт в OPENKEYS_ADMIN_USER/PASSWORD, дополнительные —
 * в OPENKEYS_ADMIN_ACCOUNTS как `user:password`, разделённые запятой или переводом строки.
 * Пароль может содержать двоеточия: делим по первому.
 */
function parseAdminAccounts(): AdminAccount[] {
  const accounts: AdminAccount[] = [];
  const primaryUser = process.env.OPENKEYS_ADMIN_USER;
  const primaryPassword = process.env.OPENKEYS_ADMIN_PASSWORD;
  if (primaryUser && primaryPassword) {
    if (primaryUser.length > 128 || primaryPassword.length > 1024) throw new Error("admin credentials are too long");
    accounts.push({ user: primaryUser, password: primaryPassword });
  }

  for (const entry of (process.env.OPENKEYS_ADMIN_ACCOUNTS ?? "").split(/[,\n]/)) {
    const trimmed = entry.trim();
    if (trimmed === "") continue;

    const separator = trimmed.indexOf(":");
    if (separator <= 0 || separator === trimmed.length - 1) {
      throw new Error("OPENKEYS_ADMIN_ACCOUNTS entries must look like user:password");
    }
    const user = trimmed.slice(0, separator);
    const password = trimmed.slice(separator + 1);
    if (user.length > 128 || password.length > 1024) throw new Error("admin credentials are too long");
    if (accounts.some((account) => account.user === user)) continue;
    accounts.push({ user, password });
  }

  if (accounts.length === 0) throw new Error("at least one admin account is required");
  return accounts;
}

export function loadConfig(): OpenkeysConfig {
  // 10000 = ключ несёт баланс выбранного API один к одному: $50 номинала — это
  // ровно $50 работы по его прайсу. Никаких скидочных тиров здесь нет.
  const defaultMultBp = optionalInt("OPENKEYS_DEFAULT_MULT_BP", 10_000);
  if (defaultMultBp < 1 || defaultMultBp > 10_000) {
    throw new Error("OPENKEYS_DEFAULT_MULT_BP must be between 1 and 10000");
  }

  const sessionSecret = required("OPENKEYS_SESSION_SECRET");
  if (sessionSecret.length < 32) throw new Error("OPENKEYS_SESSION_SECRET must be at least 32 chars");

  const sessionTtlSeconds = optionalInt("OPENKEYS_SESSION_TTL_SECONDS", 12 * 60 * 60);
  if (sessionTtlSeconds < 300 || sessionTtlSeconds > 7 * 24 * 60 * 60) {
    throw new Error("OPENKEYS_SESSION_TTL_SECONDS must be between 300 and 604800");
  }

  return {
    databaseUrl: required("OPENKEYS_DATABASE_URL"),
    engineBaseUrl: baseUrl("ENGINE_BASE_URL", "http://127.0.0.1:8790", true),
    engineControlKey: required("ENGINE_CONTROL_KEY"),
    enginePublicBaseUrl: baseUrl("ENGINE_PUBLIC_BASE_URL", "https://api.apitoken.sale"),
    engineOpenAiPublicBaseUrl: baseUrl("ENGINE_OPENAI_PUBLIC_BASE_URL", "https://openai.api.apitoken.sale"),
    adminAccounts: parseAdminAccounts(),
    sessionSecret,
    sessionTtlSeconds,
    defaultMultBp,
    publicBaseUrl: baseUrl("OPENKEYS_PUBLIC_BASE_URL", "https://openkeys.apitoken.sale"),
  };
}
