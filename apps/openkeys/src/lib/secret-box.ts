import "server-only";
import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";

/**
 * Секрет ключа лежит в базе зашифрованным: склад должен уметь отдать его
 * покупателю через день после выпуска, а класть `sk-pool-…` в базу открытым
 * текстом нельзя. AES-256-GCM даёт и шифрование, и защиту от подмены строки.
 */
const ALGORITHM = "aes-256-gcm";
const NONCE_BYTES = 12;

function keyring(): Map<string, Buffer> {
  const entries = new Map<string, Buffer>();
  if (process.env.OPENKEYS_SECRET_KEY) entries.set("legacy", Buffer.from(process.env.OPENKEYS_SECRET_KEY.trim(), "hex"));
  for (const item of (process.env.OPENKEYS_SECRET_KEYS ?? "").split(",")) {
    if (!item.trim()) continue;
    const separator = item.indexOf(":");
    const kid = item.slice(0, separator);
    const raw = item.slice(separator + 1);
    if (separator < 1 || !/^[A-Za-z0-9_-]{1,32}$/.test(kid)) throw new Error("invalid OPENKEYS_SECRET_KEYS entry");
    entries.set(kid, Buffer.from(raw.trim(), "hex"));
  }
  if (entries.size === 0) throw new Error("OPENKEYS_SECRET_KEY or OPENKEYS_SECRET_KEYS is required");
  for (const key of entries.values()) {
    if (key.length !== 32) throw new Error("OpenKeys encryption keys must be 32 bytes of hex (64 characters)");
  }
  return entries;
}

function activeKey(): { keyId: string; key: Buffer } {
  const keys = keyring();
  const keyId = process.env.OPENKEYS_SECRET_ACTIVE_KID ?? (keys.has("legacy") ? "legacy" : [...keys.keys()][0]);
  const key = keyId ? keys.get(keyId) : undefined;
  if (!key || !keyId) throw new Error("OPENKEYS_SECRET_ACTIVE_KID is not present in the keyring");
  return { keyId, key };
}

function decryptionKey(keyId: string): Buffer {
  const key = keyring().get(keyId);
  if (!key) throw new Error(`OpenKeys encryption key ${keyId} is unavailable`);
  return key;
}

export interface SealedSecret {
  ciphertext: string;
  nonce: string;
  keyId?: string;
}

export function sealSecret(plaintext: string, associatedData?: string): SealedSecret {
  const nonce = randomBytes(NONCE_BYTES);
  const { keyId, key } = activeKey();
  const cipher = createCipheriv(ALGORITHM, key, nonce);
  if (associatedData !== undefined) cipher.setAAD(Buffer.from(associatedData, "utf8"));
  const encrypted = Buffer.concat([cipher.update(plaintext, "utf8"), cipher.final()]);
  // Тег аутентификации хранится вместе с шифротекстом: без него расшифровка не проверяема.
  return {
    ciphertext: Buffer.concat([encrypted, cipher.getAuthTag()]).toString("base64"),
    nonce: nonce.toString("base64"),
    keyId,
  };
}

/** Возвращает null, если запись повреждена или ключ шифрования сменился. */
export function openSecret(sealed: SealedSecret, associatedData?: string): string | null {
  try {
    const packed = Buffer.from(sealed.ciphertext, "base64");
    if (packed.length <= 16) return null;

    const tag = packed.subarray(packed.length - 16);
    const body = packed.subarray(0, packed.length - 16);
    const decipher = createDecipheriv(ALGORITHM, decryptionKey(sealed.keyId ?? "legacy"), Buffer.from(sealed.nonce, "base64"));
    if (associatedData !== undefined) decipher.setAAD(Buffer.from(associatedData, "utf8"));
    decipher.setAuthTag(tag);
    return Buffer.concat([decipher.update(body), decipher.final()]).toString("utf8");
  } catch {
    return null;
  }
}
