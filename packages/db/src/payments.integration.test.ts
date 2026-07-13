import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  acceptVerifiedPaidWebhook,
  claimNextCredit,
  confirmCredit,
  createDatabase,
  type Database,
  type VerifiedPaidWebhook,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("payment persistence", () => {
  let database: Database;
  let userId: string;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    await database.pool.query("INSERT INTO users (id, email) VALUES ($1, $2)", [userId, `${userId}@test.invalid`]);
  });

  afterAll(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  function paidEvent(overrides: Partial<VerifiedPaidWebhook> = {}): VerifiedPaidWebhook {
    return {
      provider: "testpay",
      providerEventId: "event-1",
      eventType: "payment.succeeded",
      providerPaymentId: "payment-1",
      userId,
      engineAccountId: "acct_integration",
      amountMinor: 2500n,
      amountNano: 25_000_000_000n,
      currency: "USD",
      payload: { safe: "fixture" },
      ...overrides,
    };
  }

  it("persists a verified payment and engine credit atomically", async () => {
    const accepted = await acceptVerifiedPaidWebhook(database, paidEvent());
    expect(accepted).toMatchObject({ duplicateEvent: false });

    const payment = await database.pool.query("SELECT status, amount_nano FROM payments");
    const credit = await database.pool.query("SELECT status, idempotency_ref FROM engine_credits");
    expect(payment.rows).toEqual([{ status: "paid", amount_nano: "25000000000" }]);
    expect(credit.rows).toEqual([{ status: "pending", idempotency_ref: "testpay:payment-1" }]);
  });

  it("deduplicates webhook deliveries and payment events", async () => {
    const first = await acceptVerifiedPaidWebhook(database, paidEvent());
    const duplicateDelivery = await acceptVerifiedPaidWebhook(database, paidEvent());
    const secondEventForSamePayment = await acceptVerifiedPaidWebhook(database, paidEvent({ providerEventId: "event-2" }));

    expect(first.duplicateEvent).toBe(false);
    expect(duplicateDelivery).toEqual({ duplicateEvent: true, paymentId: null, creditId: null });
    expect(secondEventForSamePayment.paymentId).toBe(first.paymentId);
    expect(secondEventForSamePayment.creditId).toBe(first.creditId);

    const counts = await database.pool.query(`
      SELECT
        (SELECT count(*)::int FROM payments) AS payments,
        (SELECT count(*)::int FROM engine_credits) AS credits,
        (SELECT count(*)::int FROM webhook_events) AS events
    `);
    expect(counts.rows[0]).toEqual({ payments: 1, credits: 1, events: 2 });
  });

  it("rolls back a provider payment ID reused with different money", async () => {
    await acceptVerifiedPaidWebhook(database, paidEvent());
    await expect(acceptVerifiedPaidWebhook(database, paidEvent({
      providerEventId: "event-mismatch",
      amountNano: 99_000_000_000n,
    }))).rejects.toThrow("reused with different payment data");

    const events = await database.pool.query("SELECT count(*)::int AS count FROM webhook_events");
    expect(events.rows[0]).toEqual({ count: 1 });
  });

  it("claims each credit once and records the engine balance", async () => {
    await acceptVerifiedPaidWebhook(database, paidEvent());
    const [first, second] = await Promise.all([
      claimNextCredit(database, "worker-a"),
      claimNextCredit(database, "worker-b"),
    ]);
    const claimed = first ?? second;
    expect(claimed).not.toBeNull();
    expect([first, second].filter(Boolean)).toHaveLength(1);

    await confirmCredit(database, claimed!.id, 123_456_789n);
    const row = await database.pool.query("SELECT status, engine_balance_after_nano FROM engine_credits");
    expect(row.rows).toEqual([{ status: "confirmed", engine_balance_after_nano: "123456789" }]);
  });
});
