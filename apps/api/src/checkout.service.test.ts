import { createHash, randomUUID } from "node:crypto";
import { ConfigService } from "@nestjs/config";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createCheckoutSchema } from "@claude-api/contracts";
import { createDatabase, type Database } from "@claude-api/db";
import { CryptomusProvider, PaymentProviderRegistry } from "@claude-api/payments";
import type { Environment } from "./config.js";
import { CheckoutService, parseProviderWholeUsd } from "./checkout.service.js";

const connectionString = process.env.TEST_DATABASE_URL;
const paymentId = "26109ba0-b05b-4ee0-93d1-fd62c822ce95";
const apiKey = "payment-api-key";

describe("whole USD input", () => {
  it("accepts digits and rejects numbers, points, signs, and zero", () => {
    expect(createCheckoutSchema.safeParse({ amountUsd: "37" }).success).toBe(true);
    for (const amountUsd of [37, "37.0", "1.5", "+37", "-1", "0", "01"]) {
      expect(createCheckoutSchema.safeParse({ amountUsd }).success).toBe(false);
    }
  });

  it("accepts provider zero-only fractional formatting without using floats", () => {
    expect(parseProviderWholeUsd("37")).toBe(37n);
    expect(parseProviderWholeUsd("37.00000000")).toBe(37n);
    expect(() => parseProviderWholeUsd("37.01")).toThrow("not positive whole USD");
  });
});

describe.runIf(Boolean(connectionString))("Cryptomus checkout service", () => {
  let database: Database;
  let userId: string;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    await database.pool.query("INSERT INTO users (id, email) VALUES ($1, $2)", [userId, "buyer@example.com"]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, status)
      VALUES ($1, $2, 'acct_checkout_test', 'active')
    `, [randomUUID(), userId]);
  });

  afterAll(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  it("creates, verifies, and queues one exact credit", async () => {
    let checkoutId = "";
    const provider = new CryptomusProvider({
      merchantId: "00000000-0000-4000-8000-000000000001",
      paymentApiKey: apiKey,
      callbackUrl: "https://backend.apitoken.sale/v1/payments/cryptomus/webhook",
      apiBaseUrl: "https://api.test",
      fetch: async (input, init) => {
        const url = String(input);
        const request = JSON.parse(String(init?.body)) as Record<string, unknown>;
        if (url.endsWith("/v1/payment")) {
          checkoutId = String(request.order_id);
          expect(request.amount).toBe("37");
          return Response.json(apiPayment("check", checkoutId));
        }
        expect(url).toContain("/v1/payment/info");
        expect(request).toEqual({ uuid: paymentId });
        return Response.json(apiPayment("paid", checkoutId));
      },
    });
    const config = new ConfigService<Environment, true>({
      MIN_TOPUP_USD: 1n,
      MAX_TOPUP_USD: 10_000n,
      PUBLIC_APP_BASE_URL: "https://apitoken.sale",
    } as Environment);
    const service = new CheckoutService(database, new PaymentProviderRegistry([provider]), config);

    const created = await service.create(userId, "37", "cryptomus");
    expect(created).toMatchObject({ amountUsd: "37", status: "pending", checkoutUrl: `https://pay.test/${paymentId}` });

    const unsigned = { type: "payment", uuid: paymentId, order_id: checkoutId, status: "paid" };
    const webhook = JSON.stringify({ ...unsigned, sign: signWebhook(unsigned) });
    const applied = await service.processCryptomusWebhook(webhook);
    expect(applied).toMatchObject({ duplicateEvent: false, checkoutStatus: "paid" });
    const credit = await database.pool.query("SELECT amount_nano, engine_account_id FROM engine_credits");
    expect(credit.rows).toEqual([{ amount_nano: "37000000000", engine_account_id: "acct_checkout_test" }]);
  });
});

function apiPayment(status: string, checkoutId: string): Record<string, unknown> {
  return {
    state: 0,
    result: {
      uuid: paymentId,
      order_id: checkoutId,
      amount: "37.00",
      currency: "USD",
      payment_status: status,
      status,
      url: `https://pay.test/${paymentId}`,
      is_final: status === "paid",
      additional_data: JSON.stringify({ checkoutId }),
      updated_at: "2026-07-13T15:00:00+03:00",
      expired_at: 1_783_955_600,
    },
  };
}

function signWebhook(value: unknown): string {
  const json = JSON.stringify(value).replace(/\//g, "\\/");
  return createHash("md5").update(`${Buffer.from(json).toString("base64")}${apiKey}`).digest("hex");
}
