import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { listAdminUserOverview } from "./admin-overview.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("admin user overview pricing bundle", () => {
  let database: Database;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query("TRUNCATE users RESTART IDENTITY CASCADE");
  });

  afterAll(async () => {
    await database.pool.end();
  });

  it("returns one B2B client and one total for a default plus provider pricing jobs", async () => {
    const userId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, 'buyer@test.invalid', 'Buyer')",
      [userId],
    );
    await database.pool.query(`
      INSERT INTO customer_profiles
        (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2b', NULL, 3700, date_trunc('month', now()))
    `, [userId]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, 'acct-buyer', 3700, 'active')
    `, [randomUUID(), userId]);
    await database.pool.query(`
      INSERT INTO engine_pricing_jobs (
        id, user_id, engine_account_id, provider_id, multiplier_bp, reason, status,
        attempts, last_error, confirmed_at
      ) VALUES
        ($1, $4, 'acct-buyer', NULL, 3700, 'default', 'confirmed', 1, NULL, now() - interval '2 minutes'),
        ($2, $4, 'acct-buyer', 'openai', 4100, 'provider', 'processing', 2, NULL, NULL),
        ($3, $4, 'acct-buyer', 'google', 3900, 'provider', 'retry', 4, 'engine unavailable', NULL)
    `, [randomUUID(), randomUUID(), randomUUID(), userId]);

    const page = await listAdminUserOverview(database, { customerType: "b2b" });

    expect(page.total).toBe(1);
    expect(page.rows).toHaveLength(1);
    expect(page.rows[0]).toMatchObject({
      id: userId,
      multiplierBp: 3700,
      pricingSyncStatus: "retry",
      pricingSyncAttempts: 4,
      pricingSyncError: "engine unavailable",
      pricingSyncConfirmedAt: null,
    });
  });
});
