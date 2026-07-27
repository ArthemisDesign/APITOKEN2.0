import { randomUUID } from "node:crypto";
import { B2C_PRICING_TIERS } from "@claude-api/contracts";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  applyTopupTier,
  closeElapsedTierWindows,
  completePricingUsageSync,
  createDatabase,
  getPricingUsageCursor,
  type Database,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;
const NOW = new Date("2026-07-27T12:00:00.000Z");
const HOLD_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;

describe.runIf(Boolean(connectionString))("canonical B2C tier persistence", () => {
  let database: Database;
  let ledgerId = 0;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query("TRUNCATE webhook_events, users RESTART IDENTITY CASCADE");
    ledgerId = 0;
  });

  afterAll(async () => {
    await database.pool.query("TRUNCATE webhook_events, users RESTART IDENTITY CASCADE");
    await database.pool.end();
  });

  async function createPricingUser(input: {
    label: string;
    tier: number;
    cumulativeNano: bigint;
    windowStart: Date | null;
    cursorComplete?: boolean;
  }): Promise<{ userId: string; engineAccountId: string }> {
    const userId = randomUUID();
    const engineAccountId = `acct-${input.label}-${userId}`;
    const multiplierBp = B2C_PRICING_TIERS[input.tier]!.multiplierBp;
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)",
      [userId, `${input.label}-${userId}@test.invalid`, `Pricing ${input.label}`],
    );
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, $4, 'active')
    `, [randomUUID(), userId, engineAccountId, multiplierBp]);
    await database.pool.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start,
        cumulative_topup_nano, tier_window_start, tier_window_spent_nano
      ) VALUES ($1, 'b2c', $2, $3, '2026-07-01T00:00:00Z', $4, $5, 0)
    `, [userId, input.tier, multiplierBp, input.cumulativeNano.toString(), input.windowStart]);
    await database.pool.query(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id, last_ledger_id, updated_at)
      VALUES ($1, $2, 0, $3)
    `, [engineAccountId, userId, input.cursorComplete === false ? "-infinity" : NOW]);
    return { userId, engineAccountId };
  }

  async function insertUsage(
    userId: string,
    engineAccountId: string,
    amountNano: bigint,
    occurredAt: Date,
  ): Promise<void> {
    ledgerId += 1;
    await database.pool.query(`
      INSERT INTO pricing_usage_events (
        id, user_id, engine_account_id, ledger_entry_id, amount_nano,
        real_funded_nano, occurred_at
      ) VALUES ($1, $2, $3, $4, $5, $5, $6)
    `, [randomUUID(), userId, engineAccountId, ledgerId, amountNano.toString(), occurredAt]);
  }

  async function insertConfirmedTopup(
    userId: string,
    engineAccountId: string,
    amountNano: bigint,
  ): Promise<string> {
    const checkoutId = randomUUID();
    const paymentId = randomUUID();
    const creditId = randomUUID();
    const amountUsd = amountNano / 1_000_000_000n;
    await database.pool.query(`
      INSERT INTO checkout_sessions (
        id, user_id, engine_account_id, provider, amount_usd, amount_nano,
        provider_payment_id, status, provider_state
      ) VALUES ($1, $2, $3, 'cryptomus', $4, $5, $6, 'paid', '{}'::jsonb)
    `, [checkoutId, userId, engineAccountId, amountUsd.toString(), amountNano.toString(), `provider-${paymentId}`]);
    await database.pool.query(`
      INSERT INTO payments (
        id, checkout_id, user_id, provider, provider_payment_id, amount_minor,
        currency, amount_nano, status, provider_state, paid_at
      ) VALUES ($1, $2, $3, 'cryptomus', $4, $5, 'USD', $6, 'paid', '{}'::jsonb, now())
    `, [
      paymentId,
      checkoutId,
      userId,
      `provider-${paymentId}`,
      (amountUsd * 100n).toString(),
      amountNano.toString(),
    ]);
    await database.pool.query(`
      INSERT INTO engine_credits (
        id, payment_id, engine_account_id, amount_nano, idempotency_ref,
        status, confirmed_at
      ) VALUES ($1, $2, $3, $4, $5, 'confirmed', now())
    `, [creditId, paymentId, engineAccountId, amountNano.toString(), `cryptomus:provider-${paymentId}`]);
    return paymentId;
  }

  async function pricingState(userId: string): Promise<Record<string, unknown>> {
    const result = await database.pool.query(`
      SELECT cp.current_tier, cp.multiplier_bp, cp.cumulative_topup_nano::text,
             cp.tier_window_start, cp.tier_window_spent_nano::text,
             ea.mult_bp AS engine_multiplier_bp,
             job.reason AS job_reason, job.multiplier_bp AS job_multiplier_bp,
             job.status AS job_status
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      LEFT JOIN engine_pricing_jobs job ON job.user_id = cp.user_id
      WHERE cp.user_id = $1
    `, [userId]);
    return result.rows[0]!;
  }

  it("advances from confirmed top-ups and reverses a refund exactly once", async () => {
    const target = await createPricingUser({
      label: "topup-refund",
      tier: 0,
      cumulativeNano: 0n,
      windowStart: null,
    });
    const amountNano = B2C_PRICING_TIERS[2]!.spendThresholdNano;
    const paymentId = await insertConfirmedTopup(target.userId, target.engineAccountId, amountNano);

    await applyTopupTier(database, { engineAccountId: target.engineAccountId, amountNano });
    await applyTopupTier(database, { engineAccountId: target.engineAccountId, amountNano });
    await expect(pricingState(target.userId)).resolves.toMatchObject({
      current_tier: 2,
      multiplier_bp: B2C_PRICING_TIERS[2]!.multiplierBp,
      cumulative_topup_nano: amountNano.toString(),
      engine_multiplier_bp: B2C_PRICING_TIERS[2]!.multiplierBp,
      job_reason: "b2c_topup",
      job_multiplier_bp: B2C_PRICING_TIERS[2]!.multiplierBp,
      job_status: "pending",
    });
    const appliedMarker = await database.pool.query(
      "SELECT count(*)::int AS count FROM pricing_credit_accruals",
    );
    expect(appliedMarker.rows).toEqual([{ count: 1 }]);

    await database.pool.query("UPDATE payments SET status = 'refunded' WHERE id = $1", [paymentId]);
    await applyTopupTier(database, { engineAccountId: target.engineAccountId, amountNano });
    await applyTopupTier(database, { engineAccountId: target.engineAccountId, amountNano });

    await expect(pricingState(target.userId)).resolves.toMatchObject({
      current_tier: 0,
      multiplier_bp: B2C_PRICING_TIERS[0]!.multiplierBp,
      cumulative_topup_nano: "0",
      tier_window_start: null,
      engine_multiplier_bp: B2C_PRICING_TIERS[0]!.multiplierBp,
      job_reason: "b2c_refund_reversal",
      job_multiplier_bp: B2C_PRICING_TIERS[0]!.multiplierBp,
      job_status: "pending",
    });
    const reversalState = await database.pool.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_credit_accruals) AS accruals,
        (SELECT count(*)::int FROM engine_pricing_jobs WHERE user_id = $1) AS jobs,
        (SELECT count(*)::int FROM webhook_events WHERE provider = 'pricing-worker') AS legacy_markers
    `, [target.userId]);
    expect(reversalState.rows).toEqual([{ accruals: 0, jobs: 1, legacy_markers: 0 }]);
  });

  it("retains a tier at the exact hold threshold and advances the window once", async () => {
    const windowStart = new Date(NOW.getTime() - 31 * 24 * 60 * 60 * 1000);
    const target = await createPricingUser({
      label: "held",
      tier: 1,
      cumulativeNano: B2C_PRICING_TIERS[1]!.spendThresholdNano,
      windowStart,
    });
    await insertUsage(
      target.userId,
      target.engineAccountId,
      B2C_PRICING_TIERS[1]!.holdNano,
      new Date(windowStart.getTime() + 24 * 60 * 60 * 1000),
    );

    await expect(closeElapsedTierWindows(database, NOW, [target.userId])).resolves.toBe(1);
    const state = await pricingState(target.userId);
    expect(state).toMatchObject({
      current_tier: 1,
      multiplier_bp: B2C_PRICING_TIERS[1]!.multiplierBp,
      cumulative_topup_nano: B2C_PRICING_TIERS[1]!.spendThresholdNano.toString(),
      tier_window_spent_nano: "0",
      job_reason: null,
    });
    expect((state.tier_window_start as Date).toISOString())
      .toBe(new Date(windowStart.getTime() + HOLD_WINDOW_MS).toISOString());
  });

  it("drops exactly one tier one nano below hold and carries post-cutoff usage forward", async () => {
    const windowStart = new Date(NOW.getTime() - 31 * 24 * 60 * 60 * 1000);
    const windowEnd = new Date(windowStart.getTime() + HOLD_WINDOW_MS);
    const target = await createPricingUser({
      label: "downgrade",
      tier: 2,
      cumulativeNano: B2C_PRICING_TIERS[3]!.spendThresholdNano,
      windowStart,
    });
    await insertUsage(
      target.userId,
      target.engineAccountId,
      B2C_PRICING_TIERS[2]!.holdNano - 1n,
      new Date(windowStart.getTime() + 24 * 60 * 60 * 1000),
    );
    await insertUsage(
      target.userId,
      target.engineAccountId,
      7_000_000_000n,
      new Date(windowEnd.getTime() + 12 * 60 * 60 * 1000),
    );

    await expect(closeElapsedTierWindows(database, NOW, [target.userId])).resolves.toBe(1);
    const state = await pricingState(target.userId);
    expect(state).toMatchObject({
      current_tier: 1,
      multiplier_bp: B2C_PRICING_TIERS[1]!.multiplierBp,
      cumulative_topup_nano: B2C_PRICING_TIERS[1]!.spendThresholdNano.toString(),
      tier_window_spent_nano: "7000000000",
      engine_multiplier_bp: B2C_PRICING_TIERS[1]!.multiplierBp,
      job_reason: "b2c_window_downgrade",
      job_multiplier_bp: B2C_PRICING_TIERS[1]!.multiplierBp,
      job_status: "pending",
    });
    expect((state.tier_window_start as Date).toISOString()).toBe(windowEnd.toISOString());
  });

  it("terminates retention at tier zero with no remaining window", async () => {
    const windowStart = new Date(NOW.getTime() - 31 * 24 * 60 * 60 * 1000);
    const target = await createPricingUser({
      label: "tier-zero",
      tier: 1,
      cumulativeNano: B2C_PRICING_TIERS[1]!.spendThresholdNano,
      windowStart,
    });

    await expect(closeElapsedTierWindows(database, NOW, [target.userId])).resolves.toBe(1);
    await expect(closeElapsedTierWindows(database, NOW, [target.userId])).resolves.toBe(0);
    await expect(pricingState(target.userId)).resolves.toMatchObject({
      current_tier: 0,
      multiplier_bp: B2C_PRICING_TIERS[0]!.multiplierBp,
      cumulative_topup_nano: "0",
      tier_window_start: null,
      tier_window_spent_nano: "0",
      engine_multiplier_bp: B2C_PRICING_TIERS[0]!.multiplierBp,
      job_reason: "b2c_window_downgrade",
    });
  });

  it("defers closure after cursor invalidation and accepts a terminal empty ledger page", async () => {
    const windowStart = new Date(NOW.getTime() - 31 * 24 * 60 * 60 * 1000);
    const target = await createPricingUser({
      label: "empty-ledger",
      tier: 1,
      cumulativeNano: B2C_PRICING_TIERS[1]!.spendThresholdNano,
      windowStart,
    });

    await expect(getPricingUsageCursor(database, target)).resolves.toBe(0n);
    await expect(closeElapsedTierWindows(database, NOW, [target.userId])).resolves.toBe(0);
    await completePricingUsageSync(database, target);
    await expect(closeElapsedTierWindows(database, NOW, [target.userId])).resolves.toBe(1);
  });

  it("serializes concurrent closers so every eligible profile closes exactly once", async () => {
    const windowStart = new Date(NOW.getTime() - 31 * 24 * 60 * 60 * 1000);
    const targets = await Promise.all(["race-a", "race-b", "race-c"].map((label) =>
      createPricingUser({
        label,
        tier: 1,
        cumulativeNano: B2C_PRICING_TIERS[1]!.spendThresholdNano,
        windowStart,
      }),
    ));
    const userIds = targets.map((target) => target.userId);

    const closed = await Promise.all([
      closeElapsedTierWindows(database, NOW, userIds),
      closeElapsedTierWindows(database, NOW, userIds),
    ]);
    expect(closed[0]! + closed[1]!).toBe(3);

    const stored = await database.pool.query(`
      SELECT current_tier, cumulative_topup_nano::text, tier_window_start
      FROM customer_profiles
      WHERE user_id = ANY($1::uuid[])
      ORDER BY user_id
    `, [userIds]);
    expect(stored.rows).toHaveLength(3);
    expect(stored.rows).toEqual(stored.rows.map(() => ({
      current_tier: 0,
      cumulative_topup_nano: "0",
      tier_window_start: null,
    })));
    const jobs = await database.pool.query(
      "SELECT count(*)::int AS count FROM engine_pricing_jobs WHERE user_id = ANY($1::uuid[])",
      [userIds],
    );
    expect(jobs.rows).toEqual([{ count: 3 }]);
  });
});
