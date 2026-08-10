import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { setBusinessPricingBundle } from "./pricing.js";
import { listCustomerProviderDiscounts } from "./pricing-discounts.js";

const connectionString = process.env.TEST_DATABASE_URL;

// Скидка B2B — это ОДНИ условия: дефолт аккаунта плюс переопределения по провайдерам. Раньше
// админская правка писала дефолт одной транзакцией, а каждого провайдера — своей, так что отказ
// на середине навсегда оставлял клиента на смеси старых и новых условий. Плюс до 2026-08-10 у
// per-provider скидок вообще не было тестов, хотя это ядро новой модели.
describe.runIf(Boolean(connectionString))("negotiated B2B terms commit as one fact", () => {
  let db: Database;
  let userId: string;
  const engineAccountId = "acct_bundle";

  beforeAll(async () => {
    db = createDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });
  afterAll(async () => {
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query(
      `TRUNCATE customer_profiles, customer_provider_discounts, engine_pricing_jobs,
       engine_accounts, audit_log, users RESTART IDENTITY CASCADE`,
    );
    userId = randomUUID();
    await db.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'B')", [
      userId, `${userId}@t.invalid`,
    ]);
    await db.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status, mult_bp) VALUES ($1, $2, $3, 'active', 10000)",
      [randomUUID(), userId, engineAccountId],
    );
    await db.pool.query(
      `INSERT INTO customer_profiles (user_id, customer_type, multiplier_bp, pricing_month_start)
       VALUES ($1, 'b2b', 10000, date_trunc('month', now()))`,
      [userId],
    );
  });

  async function defaultMultiplier(): Promise<{ profile: number; mirror: number }> {
    const profile = await db.pool.query<{ multiplier_bp: number }>(
      "SELECT multiplier_bp FROM customer_profiles WHERE user_id = $1", [userId],
    );
    const mirror = await db.pool.query<{ mult_bp: number }>(
      "SELECT mult_bp FROM engine_accounts WHERE user_id = $1", [userId],
    );
    return { profile: profile.rows[0]!.multiplier_bp, mirror: mirror.rows[0]!.mult_bp };
  }

  it("writes the default and every override together, one delivery job each", async () => {
    const { jobIds } = await setBusinessPricingBundle(db, {
      userId,
      multiplierBp: 4_000,
      providers: { google: 4_500, openai: 2_500 },
      actorId: "admin-1",
      reason: "negotiated",
    });
    expect(jobIds).toHaveLength(3);
    expect(await defaultMultiplier()).toEqual({ profile: 4_000, mirror: 4_000 });
    expect(await listCustomerProviderDiscounts(db, userId)).toEqual([
      { providerId: "google", multiplierBp: 4_500 },
      { providerId: "openai", multiplierBp: 2_500 },
    ]);
    // Дефолт и провайдер — независимые доставки: они не должны вытеснять друг друга из очереди.
    const jobs = await db.pool.query<{ provider_id: string | null; multiplier_bp: number }>(
      "SELECT provider_id, multiplier_bp FROM engine_pricing_jobs WHERE user_id = $1 ORDER BY provider_id NULLS FIRST",
      [userId],
    );
    expect(jobs.rows).toEqual([
      { provider_id: null, multiplier_bp: 4_000 },
      { provider_id: "google", multiplier_bp: 4_500 },
      { provider_id: "openai", multiplier_bp: 2_500 },
    ]);
  });

  it("rolls the whole deal back when one provider is rejected", async () => {
    await expect(setBusinessPricingBundle(db, {
      userId,
      multiplierBp: 4_000,
      providers: { google: 4_500, nonesuch: 3_000 },
      actorId: "admin-1",
      reason: "typo",
    })).rejects.toThrow(/unknown provider id/);
    expect(await defaultMultiplier()).toEqual({ profile: 10_000, mirror: 10_000 });
    expect(await listCustomerProviderDiscounts(db, userId)).toEqual([]);
    const jobs = await db.pool.query("SELECT 1 FROM engine_pricing_jobs WHERE user_id = $1", [userId]);
    expect(jobs.rowCount).toBe(0);
  });

  it("clears one override without touching the rest of the deal", async () => {
    await setBusinessPricingBundle(db, {
      userId,
      multiplierBp: 4_000,
      providers: { google: 4_500, openai: 2_500 },
      actorId: "admin-1",
      reason: "negotiated",
    });
    await setBusinessPricingBundle(db, {
      userId,
      providers: { google: null },
      actorId: "admin-1",
      reason: "provider dropped",
    });
    expect(await defaultMultiplier()).toEqual({ profile: 4_000, mirror: 4_000 });
    expect(await listCustomerProviderDiscounts(db, userId)).toEqual([
      { providerId: "openai", multiplierBp: 2_500 },
    ]);
  });

  it("refuses an empty mutation instead of silently reporting success", async () => {
    await expect(setBusinessPricingBundle(db, {
      userId, actorId: "admin-1", reason: "nothing",
    })).rejects.toThrow(/empty/);
  });
});
