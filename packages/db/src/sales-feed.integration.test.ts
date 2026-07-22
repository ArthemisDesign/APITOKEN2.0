import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { listPaidTopupsAfter, listUsageEventsAfter } from "./sales-feed.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("referral-only sales feeds", () => {
  let database: Database;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE referral_attributions, pricing_usage_events, payments, checkout_sessions, users
      RESTART IDENTITY CASCADE
    `);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  async function insertUser(referred: boolean): Promise<string> {
    const userId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Sales Feed Test')",
      [userId, `${userId}@test.invalid`],
    );
    if (referred) {
      await database.pool.query(
        "INSERT INTO referral_attributions (user_id, code, created_at) VALUES ($1, 'partner-code', now() - interval '1 minute')",
        [userId],
      );
    }
    return userId;
  }

  async function insertUsage(userId: string, ledgerEntryId: number, occurredAt: Date): Promise<void> {
    await database.pool.query(`
      INSERT INTO pricing_usage_events
        (id, user_id, engine_account_id, ledger_entry_id, amount_nano, real_funded_nano, occurred_at, created_at)
      VALUES ($1, $2, $3, $4, 1000, 750, $5, now() - interval '1 minute')
    `, [randomUUID(), userId, `acct-${ledgerEntryId}`, ledgerEntryId, occurredAt]);
  }

  async function insertPaidTopup(userId: string, suffix: string, paidAt: Date): Promise<string> {
    const checkoutId = randomUUID();
    const paymentId = randomUUID();
    await database.pool.query(`
      INSERT INTO checkout_sessions
        (id, user_id, engine_account_id, provider, amount_usd, amount_nano, status, created_at)
      VALUES ($1, $2, $3, 'test', 1, 1000000000, 'paid', now() - interval '1 minute')
    `, [checkoutId, userId, `acct-${suffix}`]);
    await database.pool.query(`
      INSERT INTO payments
        (id, checkout_id, user_id, provider, provider_payment_id, amount_minor, currency,
         amount_nano, status, paid_at, created_at)
      VALUES ($1, $2, $3, 'test', $4, 100, 'USD', 1000000000, 'paid', $5, now() - interval '1 minute')
    `, [paymentId, checkoutId, userId, `payment-${suffix}`, paidAt]);
    return paymentId;
  }

  it("excludes ordinary customer spend before and after referred spend", async () => {
    const ordinaryBefore = await insertUser(false);
    const referred = await insertUser(true);
    const ordinaryAfter = await insertUser(false);
    const occurredAt = new Date(Date.now() - 60_000);

    await insertUsage(ordinaryBefore, 1, occurredAt);
    await insertUsage(referred, 2, occurredAt);
    await insertUsage(ordinaryAfter, 3, occurredAt);

    const rows = await listUsageEventsAfter(database, 0n, 100);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ userId: referred, amountNano: 750n });
  });

  it("excludes ordinary customer top-ups while preserving referred top-ups", async () => {
    const ordinaryBefore = await insertUser(false);
    const referred = await insertUser(true);
    const ordinaryAfter = await insertUser(false);
    const base = Date.now() - 120_000;

    await insertPaidTopup(ordinaryBefore, "ordinary-before", new Date(base));
    const referredPaymentId = await insertPaidTopup(referred, "referred", new Date(base + 1_000));
    await insertPaidTopup(ordinaryAfter, "ordinary-after", new Date(base + 2_000));

    const rows = await listPaidTopupsAfter(database, 0n, 100);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ userId: referred, paymentId: referredPaymentId, amountNano: 1_000_000_000n });
  });
});
