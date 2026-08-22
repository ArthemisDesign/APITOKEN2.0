import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  applySalesPartnerBusinessPricing,
  PartnerBusinessPricingAuthorizationError,
  PartnerBusinessPricingConflictError,
  PartnerBusinessPricingRequestError,
} from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("durable Sales partner B2B pricing operations", () => {
  let database: Database;
  let userId: string;
  const referralCode = "partner-owner";

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(
      "TRUNCATE referral_attributions, customer_provider_discounts, engine_pricing_jobs, customer_profiles, engine_accounts, audit_log, users RESTART IDENTITY CASCADE",
    );
    userId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Referral')",
      [userId, userId + "@test.invalid"],
    );
    await database.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status, mult_bp) VALUES ($1, $2, $3, 'active', 5000)",
      [randomUUID(), userId, "acct_" + userId],
    );
    await database.pool.query(
      "INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start, referral_floor_bps) VALUES ($1, 'b2c', 1, 5000, date_trunc('month', now()), 1500)",
      [userId],
    );
    await database.pool.query(
      "INSERT INTO referral_attributions (user_id, code) VALUES ($1, $2)",
      [userId, referralCode],
    );
  });

  afterAll(async () => {
    await database.pool.end();
  });

  function operation(overrides: Record<string, unknown> = {}) {
    return {
      operationRef: "partner-effect:00000000-0000-4000-8000-000000000001",
      userId,
      referralCode,
      ceilingPercent: 70,
      discountPercent: 60,
      providers: { kimi: 20 },
      actorId: "admin:operator@example.com",
      reason: "approved B2B request 0001",
      ...overrides,
    };
  }

  async function counts() {
    const result = await database.pool.query<{
      jobs: number;
      component_audits: number;
      operations: number;
    }>(
      "SELECT (SELECT count(*)::int FROM engine_pricing_jobs WHERE user_id = $1) AS jobs, (SELECT count(*)::int FROM audit_log WHERE target_type = 'user' AND target_id = $1::text AND action IN ('pricing.b2b_converted', 'pricing.b2b_changed', 'pricing.provider_discount_changed')) AS component_audits, (SELECT count(*)::int FROM audit_log WHERE target_type = 'sales_partner_pricing_operation') AS operations",
      [userId],
    );
    return result.rows[0]!;
  }

  it("commits conversion, provider terms, jobs and real-actor evidence as one operation", async () => {
    const result = await applySalesPartnerBusinessPricing(database, operation());
    expect(result).toEqual({
      operationRef: "partner-effect:00000000-0000-4000-8000-000000000001",
      idempotentReplay: false,
      userId,
      converted: true,
      customerType: "b2b",
      discountPercent: 60,
      providers: { kimi: 20 },
    });
    const state = await database.pool.query<{
      customer_type: string;
      current_tier: number | null;
      multiplier_bp: number;
      referral_floor_bps: number;
      mirror_bp: number;
    }>(
      "SELECT cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.referral_floor_bps, ea.mult_bp AS mirror_bp FROM customer_profiles cp JOIN engine_accounts ea ON ea.user_id = cp.user_id WHERE cp.user_id = $1",
      [userId],
    );
    expect(state.rows[0]).toEqual({
      customer_type: "b2b",
      current_tier: null,
      multiplier_bp: 4000,
      referral_floor_bps: 0,
      mirror_bp: 4000,
    });
    expect(await counts()).toEqual({ jobs: 2, component_audits: 2, operations: 1 });
    const evidence = await database.pool.query<{
      actor_type: string;
      actor_id: string;
      metadata: Record<string, unknown>;
    }>(
      "SELECT actor_type, actor_id, metadata FROM audit_log WHERE target_type = 'sales_partner_pricing_operation'",
    );
    expect(evidence.rows[0]).toMatchObject({
      actor_type: "sales",
      actor_id: "admin:operator@example.com",
      metadata: {
        requestDigest: expect.stringMatching(/^sha256:v1:[0-9a-f]{64}$/),
        request: { referralCode, actorId: "admin:operator@example.com" },
        result: { userId, converted: true },
      },
    });
  });

  it("returns stored output on exact retry without repeating any side effect", async () => {
    const exact = operation({ providers: { openai: 10, kimi: 20 } });
    const first = await applySalesPartnerBusinessPricing(database, exact);
    const replay = await applySalesPartnerBusinessPricing(database, {
      ...exact,
      providers: { kimi: 20, openai: 10 },
    });
    expect(first.idempotentReplay).toBe(false);
    expect(replay).toEqual({ ...first, idempotentReplay: true });
    expect(await counts()).toEqual({ jobs: 3, component_audits: 3, operations: 1 });
  });

  it("serializes concurrent exact retries and commits side effects only once", async () => {
    const exact = operation({ providers: { openai: 10, kimi: 20 } });
    const [left, right] = await Promise.all([
      applySalesPartnerBusinessPricing(database, exact),
      applySalesPartnerBusinessPricing(database, {
        ...exact,
        providers: { kimi: 20, openai: 10 },
      }),
    ]);
    expect([left.idempotentReplay, right.idempotentReplay].sort()).toEqual([false, true]);
    expect(left.converted).toBe(true);
    expect(right.converted).toBe(true);
    expect(await counts()).toEqual({ jobs: 3, component_audits: 3, operations: 1 });
  });

  it("rejects operation-ref payload drift and preserves the committed terms", async () => {
    await applySalesPartnerBusinessPricing(database, operation());
    await expect(applySalesPartnerBusinessPricing(database, operation({
      discountPercent: 50,
    }))).rejects.toBeInstanceOf(PartnerBusinessPricingConflictError);
    const state = await database.pool.query<{ multiplier_bp: number }>(
      "SELECT multiplier_bp FROM customer_profiles WHERE user_id = $1",
      [userId],
    );
    expect(state.rows[0]!.multiplier_bp).toBe(4000);
    expect(await counts()).toEqual({ jobs: 2, component_audits: 2, operations: 1 });
  });

  it("fails closed on ownership, ceiling and provider-only conversion without partial writes", async () => {
    await expect(applySalesPartnerBusinessPricing(database, operation({
      operationRef: "partner-effect:wrong-owner",
      referralCode: "another-partner",
    }))).rejects.toBeInstanceOf(PartnerBusinessPricingAuthorizationError);
    await expect(applySalesPartnerBusinessPricing(database, operation({
      operationRef: "partner-effect:over-ceiling",
      ceilingPercent: 20,
    }))).rejects.toBeInstanceOf(PartnerBusinessPricingAuthorizationError);
    await expect(applySalesPartnerBusinessPricing(database, operation({
      operationRef: "partner-effect:no-default",
      discountPercent: undefined,
    }))).rejects.toBeInstanceOf(PartnerBusinessPricingRequestError);
    const state = await database.pool.query<{ customer_type: string; multiplier_bp: number }>(
      "SELECT customer_type, multiplier_bp FROM customer_profiles WHERE user_id = $1",
      [userId],
    );
    expect(state.rows[0]).toEqual({ customer_type: "b2c", multiplier_bp: 5000 });
    expect(await counts()).toEqual({ jobs: 0, component_audits: 0, operations: 0 });
  });
});
