import { createHash, createHmac, timingSafeEqual } from "node:crypto";

// Проверка подписи Telegram Login Widget (https://core.telegram.org/widgets/login):
// secret = SHA256(bot_token); hash = HMAC-SHA256(data_check_string, secret), где
// data_check_string — все поля кроме hash, отсортированные, в виде "key=value" через \n.

export interface TelegramLoginPayload {
  id: string; // telegram user id (число, но держим строкой — pg bigint)
  first_name?: string | undefined;
  last_name?: string | undefined;
  username?: string | undefined;
  photo_url?: string | undefined;
  auth_date: number;
  hash: string;
}

export const TELEGRAM_AUTH_MAX_AGE_SECONDS = 86_400;

export function verifyTelegramLogin(
  payload: TelegramLoginPayload,
  botToken: string,
  nowSeconds: number = Math.floor(Date.now() / 1000),
): boolean {
  if (!/^[0-9a-f]{64}$/.test(payload.hash)) return false;
  if (!Number.isInteger(payload.auth_date)) return false;
  if (nowSeconds - payload.auth_date > TELEGRAM_AUTH_MAX_AGE_SECONDS) return false;
  if (payload.auth_date - nowSeconds > 300) return false;

  const fields: Record<string, string> = { id: payload.id, auth_date: String(payload.auth_date) };
  if (payload.first_name !== undefined) fields.first_name = payload.first_name;
  if (payload.last_name !== undefined) fields.last_name = payload.last_name;
  if (payload.username !== undefined) fields.username = payload.username;
  if (payload.photo_url !== undefined) fields.photo_url = payload.photo_url;

  const dataCheckString = Object.keys(fields)
    .sort()
    .map((key) => `${key}=${fields[key]}`)
    .join("\n");
  const secret = createHash("sha256").update(botToken, "utf8").digest();
  const expected = createHmac("sha256", secret).update(dataCheckString, "utf8").digest();
  const supplied = Buffer.from(payload.hash, "hex");
  return supplied.length === expected.length && timingSafeEqual(supplied, expected);
}

/** Нормализация telegram-юзернейма: без @, lower. null — если пусто/невалидно. */
export function normalizeTelegramUsername(value: string | null | undefined): string | null {
  if (!value) return null;
  const stripped = value.trim().replace(/^@/, "").toLowerCase();
  return /^[a-z0-9_]{5,32}$/.test(stripped) ? stripped : null;
}
