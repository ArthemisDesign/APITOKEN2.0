import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { assertSecretBoxReady, openSecret, sealSecret } from "./secret-box";

describe("secret box", () => {
  beforeEach(() => {
    process.env.OPENKEYS_SECRET_KEY = "11".repeat(32);
  });
  afterEach(() => {
    delete process.env.OPENKEYS_SECRET_KEY;
    delete process.env.OPENKEYS_SECRET_KEYS;
    delete process.env.OPENKEYS_SECRET_ACTIVE_KID;
  });

  it("round-trips a secret bound to its record context", () => {
    const sealed = sealSecret("sk-pool-secret", "row-a");
    expect(openSecret(sealed, "row-a")).toBe("sk-pool-secret");
    expect(openSecret(sealed, "row-b")).toBeNull();
  });

  it("detects ciphertext tampering", () => {
    const sealed = sealSecret("sk-pool-secret", "row-a");
    const packed = Buffer.from(sealed.ciphertext, "base64");
    packed[0] ^= 1;
    expect(openSecret({ ...sealed, ciphertext: packed.toString("base64") }, "row-a")).toBeNull();
  });

  it("decrypts old ciphertext after rotating the active key", () => {
    process.env.OPENKEYS_SECRET_KEYS = `old:${"22".repeat(32)},new:${"33".repeat(32)}`;
    process.env.OPENKEYS_SECRET_ACTIVE_KID = "old";
    const old = sealSecret("stock-secret", "row-a");
    process.env.OPENKEYS_SECRET_ACTIVE_KID = "new";
    const current = sealSecret("new-secret", "row-b");
    expect(old.keyId).toBe("old");
    expect(current.keyId).toBe("new");
    expect(openSecret(old, "row-a")).toBe("stock-secret");
  });

  it("fails readiness for an unavailable active KID", () => {
    process.env.OPENKEYS_SECRET_KEYS = `old:${"22".repeat(32)}`;
    process.env.OPENKEYS_SECRET_ACTIVE_KID = "missing";
    expect(() => assertSecretBoxReady()).toThrow("not present in the keyring");
  });
});
