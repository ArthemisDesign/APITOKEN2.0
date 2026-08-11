import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  claimNextPricingJob,
  confirmPricingJob,
  recoverStalePricingJobs,
  setBusinessPricingBundle,
} from "./pricing.js";
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

  // Аренда джобы должна фехтоваться: воркер, у которого lease отобрали, всё ещё жив и может
  // дописать свой вердикт поверх чужой доставки — пометить confirmed то, что новый владелец
  // ещё не отправил в движок.
  it("a worker that lost its lease cannot land a verdict on the new owner's delivery", async () => {
    await setBusinessPricingBundle(db, {
      userId, multiplierBp: 4_000, actorId: "admin-1", reason: "negotiated",
    });
    const stale = await claimNextPricingJob(db, "worker-old");
    expect(stale).not.toBeNull();
    // Аренда протухла и восстановлена, джобу забрал другой воркер.
    await db.pool.query(
      "UPDATE engine_pricing_jobs SET status = 'retry', locked_at = NULL, locked_by = NULL, next_attempt_at = now() WHERE id = $1",
      [stale!.id],
    );
    const fresh = await claimNextPricingJob(db, "worker-new");
    expect(fresh!.id).toBe(stale!.id);
    expect(fresh!.attempts).toBe(stale!.attempts + 1);

    await confirmPricingJob(db, stale!);
    const after = await db.pool.query<{ status: string; locked_by: string | null }>(
      "SELECT status, locked_by FROM engine_pricing_jobs WHERE id = $1", [stale!.id],
    );
    expect(after.rows[0]).toEqual({ status: "processing", locked_by: "worker-new" });

    // Владелец аренды по-прежнему завершает её штатно.
    await confirmPricingJob(db, fresh!);
    const settled = await db.pool.query<{ status: string }>(
      "SELECT status FROM engine_pricing_jobs WHERE id = $1", [stale!.id],
    );
    expect(settled.rows[0]!.status).toBe("confirmed");
  });

  it("requeues a historical confirmed payload that no longer matches durable desired state", async () => {
    await setBusinessPricingBundle(db, {
      userId, multiplierBp: 4_000, actorId: "admin-1", reason: "negotiated",
    });
    const delivered = await claimNextPricingJob(db, "worker-old");
    expect(delivered).not.toBeNull();
    await confirmPricingJob(db, delivered!);

    // Reproduce the historical pre-requeue shape: desired state moved without touching the job.
    await db.pool.query(
      "UPDATE customer_profiles SET multiplier_bp = 5000 WHERE user_id = $1",
      [userId],
    );
    await db.pool.query(
      "UPDATE engine_accounts SET mult_bp = 5000 WHERE user_id = $1",
      [userId],
    );

    await expect(recoverStalePricingJobs(db)).resolves.toBe(1);
    const recovered = await db.pool.query<{
      status: string; multiplier_bp: number; reason: string; confirmed_at: Date | null;
    }>(`
      SELECT status, multiplier_bp, reason, confirmed_at
      FROM engine_pricing_jobs WHERE user_id = $1 AND provider_id IS NULL
    `, [userId]);
    expect(recovered.rows[0]).toEqual({
      status: "pending",
      multiplier_bp: 5_000,
      reason: "recovered_stale_confirmed",
      confirmed_at: null,
    });

    const fresh = await claimNextPricingJob(db, "worker-new");
    expect(fresh).toMatchObject({ multiplierBp: 5_000, workerId: "worker-new" });
    await confirmPricingJob(db, fresh!);
    await expect(recoverStalePricingJobs(db)).resolves.toBe(0);
  });
});
