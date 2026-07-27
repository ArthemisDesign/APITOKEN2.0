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
  return Number(raw);
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
    if (accounts.some((account) => account.user === user)) continue;
    accounts.push({ user, password: trimmed.slice(separator + 1) });
  }

  if (accounts.length === 0) throw new Error("at least one admin account is required");
  return accounts;
}

export function loadConfig(): OpenkeysConfig {
  const defaultMultBp = optionalInt("OPENKEYS_DEFAULT_MULT_BP", 4000);
  if (defaultMultBp < 1 || defaultMultBp > 10_000) {
    throw new Error("OPENKEYS_DEFAULT_MULT_BP must be between 1 and 10000");
  }

  const sessionSecret = required("OPENKEYS_SESSION_SECRET");
  if (sessionSecret.length < 32) throw new Error("OPENKEYS_SESSION_SECRET must be at least 32 chars");

  return {
    databaseUrl: required("OPENKEYS_DATABASE_URL"),
    engineBaseUrl: process.env.ENGINE_BASE_URL ?? "http://127.0.0.1:8790",
    engineControlKey: required("ENGINE_CONTROL_KEY"),
    enginePublicBaseUrl: process.env.ENGINE_PUBLIC_BASE_URL ?? "https://api.apitoken.sale",
    adminAccounts: parseAdminAccounts(),
    sessionSecret,
    sessionTtlSeconds: optionalInt("OPENKEYS_SESSION_TTL_SECONDS", 12 * 60 * 60),
    defaultMultBp,
    publicBaseUrl: process.env.OPENKEYS_PUBLIC_BASE_URL ?? "https://openkeys.apitoken.sale",
  };
}
