import { randomBytes } from "node:crypto";
import { describe, expect, it } from "vitest";
import { authenticator } from "otplib";
import { TotpService } from "./totp.service.js";

// Мок БД: одна строка users с totp_secret/totp_enabled, поведение под запросы totp.ts.
function makeService() {
  const store: { secret: string | null; enabled: boolean } = { secret: null, enabled: false };
  const db = {
    pool: {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      query: async (sql: string, params: any[]) => {
        if (sql.startsWith("SELECT")) return { rows: [{ totp_secret: store.secret, totp_enabled: store.enabled }] };
        if (sql.includes("totp_secret = $2")) { store.secret = params[1]; store.enabled = false; return { rows: [] }; }
        if (sql.includes("totp_enabled = true")) { if (store.secret) store.enabled = true; return { rows: [] }; }
        if (sql.includes("totp_secret = NULL")) { store.secret = null; store.enabled = false; return { rows: [] }; }
        return { rows: [] };
      },
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  } as any;
  const key = randomBytes(32).toString("base64url");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const config = { get: () => key } as any;
  return { service: new TotpService(db, config), store };
}

describe("TotpService", () => {
  it("setup → enable → verify accepts real codes, rejects wrong ones", async () => {
    const { service, store } = makeService();
    const setup = await service.setup("u1", "alice@example.com");
    expect(setup.secret).toMatch(/^[A-Z2-7]+$/); // base32
    expect(setup.otpauthUri).toContain("otpauth://totp/");
    expect(setup.qrDataUrl.startsWith("data:image/")).toBe(true);
    expect(store.enabled).toBe(false); // pending until verified

    await service.enable("u1", authenticator.generate(setup.secret));
    expect(store.enabled).toBe(true);

    expect(await service.verify("u1", authenticator.generate(setup.secret))).toBe(true);
    expect(await service.verify("u1", "000000")).toBe(false);
  });

  it("enable rejects an invalid code and does not enable", async () => {
    const { service, store } = makeService();
    await service.setup("u1", "bob@example.com");
    await expect(service.enable("u1", "123456")).rejects.toThrow();
    expect(store.enabled).toBe(false);
  });

  it("verify is false when 2FA is not enabled", async () => {
    const { service } = makeService();
    const setup = await service.setup("u1", "carol@example.com");
    // secret exists but not enabled yet → gate must not pass
    expect(await service.verify("u1", authenticator.generate(setup.secret))).toBe(false);
  });

  it("disable clears the secret after a valid code", async () => {
    const { service, store } = makeService();
    const setup = await service.setup("u1", "dave@example.com");
    await service.enable("u1", authenticator.generate(setup.secret));
    await service.disable("u1", authenticator.generate(setup.secret));
    expect(store.enabled).toBe(false);
    expect(store.secret).toBeNull();
  });
});
