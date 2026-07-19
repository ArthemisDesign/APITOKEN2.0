import { createHash, createHmac } from "node:crypto";
import { describe, expect, it } from "vitest";
import { normalizeTelegramUsername, verifyTelegramLogin, type TelegramLoginPayload } from "./telegram.js";

const BOT_TOKEN = "1234567890:TEST-bot-token-for-signature-check";

function sign(fields: Record<string, string>): string {
  const dataCheckString = Object.keys(fields).sort().map((key) => `${key}=${fields[key]}`).join("\n");
  const secret = createHash("sha256").update(BOT_TOKEN, "utf8").digest();
  return createHmac("sha256", secret).update(dataCheckString, "utf8").digest("hex");
}

function payload(now: number, overrides: Partial<TelegramLoginPayload> = {}): TelegramLoginPayload {
  const base = {
    id: "42424242",
    first_name: "Ada",
    username: "ada_seller",
    auth_date: now - 10,
  };
  const merged = { ...base, ...overrides };
  const fields: Record<string, string> = { id: merged.id, auth_date: String(merged.auth_date) };
  if (merged.first_name !== undefined) fields.first_name = merged.first_name;
  if (merged.last_name !== undefined) fields.last_name = merged.last_name;
  if (merged.username !== undefined) fields.username = merged.username;
  if (merged.photo_url !== undefined) fields.photo_url = merged.photo_url;
  return { ...merged, hash: overrides.hash ?? sign(fields) };
}

describe("verifyTelegramLogin", () => {
  const now = 1_784_500_000;

  it("accepts a correctly signed payload", () => {
    expect(verifyTelegramLogin(payload(now), BOT_TOKEN, now)).toBe(true);
  });

  it("rejects a tampered field", () => {
    const tampered = { ...payload(now), id: "999" };
    expect(verifyTelegramLogin(tampered, BOT_TOKEN, now)).toBe(false);
  });

  it("rejects a wrong bot token", () => {
    expect(verifyTelegramLogin(payload(now), "other-token", now)).toBe(false);
  });

  it("rejects stale auth_date (beyond the max-age window)", () => {
    expect(verifyTelegramLogin(payload(now, { auth_date: now - 90_000 }), BOT_TOKEN, now)).toBe(false);
  });

  it("rejects malformed hash", () => {
    expect(verifyTelegramLogin(payload(now, { hash: "zz" }), BOT_TOKEN, now)).toBe(false);
  });

  it("accepts payload without optional fields", () => {
    expect(verifyTelegramLogin(payload(now, { first_name: undefined, username: undefined }), BOT_TOKEN, now)).toBe(true);
  });
});

describe("normalizeTelegramUsername", () => {
  it("strips @ and lowercases", () => {
    expect(normalizeTelegramUsername("@Ada_Seller")).toBe("ada_seller");
  });
  it("rejects invalid values", () => {
    expect(normalizeTelegramUsername("a b")).toBeNull();
    expect(normalizeTelegramUsername("abc")).toBeNull();
    expect(normalizeTelegramUsername(null)).toBeNull();
  });
});
