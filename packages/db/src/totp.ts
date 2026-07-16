import type { Database } from "./client.js";

// Доступ к TOTP-состоянию пользователя. Секрет ХРАНИТСЯ ЗАШИФРОВАННЫМ (auth-secrets, AES-GCM):
// этот слой его не расшифровывает — только читает/пишет ciphertext. Дешифровка/проверка кода — в apps/api.
export interface UserTotp {
  secret: string | null; // ciphertext (или null, если не заведён)
  enabled: boolean;      // true только после подтверждения кодом
}

export async function getUserTotp(database: Database, userId: string): Promise<UserTotp | null> {
  const result = await database.pool.query<{ totp_secret: string | null; totp_enabled: boolean }>(
    "SELECT totp_secret, totp_enabled FROM users WHERE id = $1",
    [userId],
  );
  const row = result.rows[0];
  return row ? { secret: row.totp_secret, enabled: row.totp_enabled } : null;
}

// Завести/перевыпустить секрет в статусе pending (enabled=false), пока код не подтверждён.
export async function setUserTotpPending(database: Database, userId: string, encryptedSecret: string): Promise<void> {
  await database.pool.query(
    "UPDATE users SET totp_secret = $2, totp_enabled = false, updated_at = now() WHERE id = $1",
    [userId, encryptedSecret],
  );
}

// Активировать 2FA — только если секрет уже заведён.
export async function enableUserTotp(database: Database, userId: string): Promise<void> {
  await database.pool.query(
    "UPDATE users SET totp_enabled = true, updated_at = now() WHERE id = $1 AND totp_secret IS NOT NULL",
    [userId],
  );
}

// Полностью снять 2FA: убрать секрет и флаг.
export async function clearUserTotp(database: Database, userId: string): Promise<void> {
  await database.pool.query(
    "UPDATE users SET totp_secret = NULL, totp_enabled = false, updated_at = now() WHERE id = $1",
    [userId],
  );
}
