import "server-only";
import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";

/**
 * Секрет ключа лежит в базе зашифрованным: склад должен уметь отдать его
 * покупателю через день после выпуска, а класть `sk-pool-…` в базу открытым
 * текстом нельзя. AES-256-GCM даёт и шифрование, и защиту от подмены строки.
 */
const ALGORITHM = "aes-256-gcm";
const NONCE_BYTES = 12;

function encryptionKey(): Buffer {
  const raw = process.env.OPENKEYS_SECRET_KEY;
  if (!raw) throw new Error("OPENKEYS_SECRET_KEY is required to store key secrets");

  const key = Buffer.from(raw.trim(), "hex");
  if (key.length !== 32) throw new Error("OPENKEYS_SECRET_KEY must be 32 bytes of hex (64 characters)");
  return key;
}

export interface SealedSecret {
  ciphertext: string;
  nonce: string;
}

export function sealSecret(plaintext: string): SealedSecret {
  const nonce = randomBytes(NONCE_BYTES);
  const cipher = createCipheriv(ALGORITHM, encryptionKey(), nonce);
  const encrypted = Buffer.concat([cipher.update(plaintext, "utf8"), cipher.final()]);
  // Тег аутентификации хранится вместе с шифротекстом: без него расшифровка не проверяема.
  return {
    ciphertext: Buffer.concat([encrypted, cipher.getAuthTag()]).toString("base64"),
    nonce: nonce.toString("base64"),
  };
}

/** Возвращает null, если запись повреждена или ключ шифрования сменился. */
export function openSecret(sealed: SealedSecret): string | null {
  try {
    const packed = Buffer.from(sealed.ciphertext, "base64");
    if (packed.length <= 16) return null;

    const tag = packed.subarray(packed.length - 16);
    const body = packed.subarray(0, packed.length - 16);
    const decipher = createDecipheriv(ALGORITHM, encryptionKey(), Buffer.from(sealed.nonce, "base64"));
    decipher.setAuthTag(tag);
    return Buffer.concat([decipher.update(body), decipher.final()]).toString("utf8");
  } catch {
    return null;
  }
}
