import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { listAdminPayingUsers } from "./admin-finance.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("paying users provider spend", () => {
  let database: Database;
  let userId: string;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE pricing_usage_attributions, pricing_usage_events,
               payments, checkout_sessions, users RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    const checkoutId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, 'provider@test.invalid', 'Provider Test')",
      [userId],
    );
    await database.pool.query(`
      INSERT INTO checkout_sessions (
        id, user_id, engine_account_id, provider, amount_usd, amount_nano,
        provider_payment_id, status, provider_state, completed_at
      ) VALUES ($1, $2, 'acct_provider_admin', 'test', 25, 25000000000,
                'provider-admin-payment', 'paid', '{}'::jsonb, now())
    `, [checkoutId, userId]);
    await database.pool.query(`
      INSERT INTO payments (
        id, checkout_id, user_id, provider, provider_payment_id, amount_minor,
        currency, amount_nano, status, provider_state, paid_at
      ) VALUES ($1, $2, $3, 'test', 'provider-admin-payment', 2500,
                'USD', 25000000000, 'paid', '{}'::jsonb, now())
    `, [randomUUID(), checkoutId, userId]);
    await database.pool.query(`
      INSERT INTO pricing_usage_events (
        id, user_id, engine_account_id, ledger_entry_id, provider_id,
        amount_nano, occurred_at
      ) VALUES
        ($1, $5, 'acct_provider_admin', 1, 'anthropic', 100, now()),
        ($2, $5, 'acct_provider_admin', 2, 'openai', 200, now()),
        ($3, $5, 'acct_provider_admin', 3, 'google', 300, now()),
        ($4, $5, 'acct_provider_admin', 4, 'unattributed', 400, now())
    `, [randomUUID(), randomUUID(), randomUUID(), randomUUID(), userId]);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  it("aggregates persisted ledger providers without model-name inference", async () => {
    const page = await listAdminPayingUsers(database, { days: 30 });
    expect(page.rows).toHaveLength(1);
    expect(page.rows[0]).toMatchObject({
      userId,
      spentNano: "1000",
      providerSpendNano: {
        anthropic: "100",
        openai: "200",
        google: "300",
        other: "400",
      },
    });
    expect(page.summary).toMatchObject({
      payingUsers: 1,
      activeSpenders: 1,
      spentNano: "1000",
      providerSpendNano: {
        anthropic: "100",
        openai: "200",
        google: "300",
        other: "400",
      },
      providerUsers: { anthropic: 1, openai: 1, google: 1, other: 1 },
    });

    const openai = await listAdminPayingUsers(database, { days: 30, provider: "openai" });
    expect(openai.total).toBe(1);
    expect(openai.rows[0]?.userId).toBe(userId);
  });
});
