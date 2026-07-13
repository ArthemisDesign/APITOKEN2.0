import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";

export function decodeAuthEncryptionKey(value: string): Buffer {
  if (!/^[A-Za-z0-9_-]{43}$/.test(value)) throw new Error("AUTH_TOKEN_ENCRYPTION_KEY must be 32-byte base64url");
  const key = Buffer.from(value, "base64url");
  if (key.length !== 32) throw new Error("AUTH_TOKEN_ENCRYPTION_KEY must decode to 32 bytes");
  return key;
}

export function encryptAuthToken(token: string, key: Buffer): string {
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", key, iv);
  const encrypted = Buffer.concat([cipher.update(token, "utf8"), cipher.final()]);
  return ["v1", iv.toString("base64url"), encrypted.toString("base64url"), cipher.getAuthTag().toString("base64url")].join(".");
}

export function decryptAuthToken(value: string, key: Buffer): string {
  const [version, ivText, encryptedText, tagText] = value.split(".");
  if (version !== "v1" || !ivText || !encryptedText || !tagText) throw new Error("invalid encrypted auth token");
  const decipher = createDecipheriv("aes-256-gcm", key, Buffer.from(ivText, "base64url"));
  decipher.setAuthTag(Buffer.from(tagText, "base64url"));
  return Buffer.concat([
    decipher.update(Buffer.from(encryptedText, "base64url")),
    decipher.final(),
  ]).toString("utf8");
}
