import { createHash } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  BusinessCustomerNotFoundError,
  InvalidBusinessInvitationError,
  applyPricingLedgerPage,
  closeElapsedPricingMonths,
  completeEngineAccount,
  createBusinessInvite,
  createDatabase,
  createEmailUser,
  getPricingUsageCursor,
  getPricingView,
  setBusinessPricing,
  type Database,
  type PricingSyncTarget,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("progressive and business pricing", () => {
  let database: Database;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(clean);

  afterAll(async () => {
    await clean();
    await database.pool.end();
  });

  it("starts B2C customers at 60% off and upgrades exact local spend idempotently", async () => {
    const user = await createEmailUser(database, "solo@example.com", "test-password-hash");
    await completeEngineAccount(database, user.id, "acct_solo");
    const target: PricingSyncTarget = { userId: user.id, engineAccountId: "acct_solo" };

    expect(user).toMatchObject({ customerType: "b2c", engineMultiplierBp: 4000 });
    await expect(getPricingUsageCursor(database, target)).resolves.toBe(0n);

    const firstCharge = ledgerCharge("1", 25_000_000_000n);
    await applyPricingLedgerPage(database, target, [firstCharge]);
    await applyPricingLedgerPage(database, target, [firstCharge]);
    await expect(getPricingView(database, user.id)).resolves.toMatchObject({
      tier: "builder", discountPercent: 65, multiplierBp: 3500, spentNano: "25000000000",
    });
    await expect(setBusinessPricing(
      database, { userId: user.id, multiplierBp: 1000, actorId: "test-admin" },
    )).rejects.toBeInstanceOf(BusinessCustomerNotFoundError);

    await applyPricingLedgerPage(database, target, [ledgerCharge("2", 50_000_000_000n)]);
    await expect(getPricingView(database, user.id)).resolves.toMatchObject({
      tier: "pro", discountPercent: 70, multiplierBp: 3000, spentNano: "75000000000",
    });
    const state = await database.pool.query(`
      SELECT ea.mult_bp, j.multiplier_bp, j.reason,
             (SELECT count(*)::int FROM pricing_usage_events) AS event_count
      FROM engine_accounts ea JOIN engine_pricing_jobs j ON j.user_id = ea.user_id
      WHERE ea.user_id = $1
    `, [user.id]);
    expect(state.rows[0]).toMatchObject({
      mult_bp: 3000, multiplier_bp: 3000, reason: "b2c_upgrade", event_count: 2,
    });
  });

  it("drops only one retained tier when an elapsed month misses its threshold", async () => {
    const user = await createEmailUser(database, "retained@example.com", "test-password-hash");
    await completeEngineAccount(database, user.id, "acct_retained");
    const june = new Date("2026-06-01T00:00:00.000Z");
    await database.pool.query(`
      UPDATE customer_profiles
      SET current_tier = 4, multiplier_bp = 2000, pricing_month_start = $2
      WHERE user_id = $1
    `, [user.id, june]);
    await database.pool.query(`
      INSERT INTO pricing_months (id, user_id, month_start, opening_tier, highest_tier, spent_nano)
      VALUES ('11111111-1111-1111-1111-111111111111', $1, $2, 4, 4, 499000000000)
      ON CONFLICT (user_id, month_start) DO UPDATE SET spent_nano = EXCLUDED.spent_nano
    `, [user.id, june]);

    await expect(closeElapsedPricingMonths(database, new Date("2026-07-02T00:00:00.000Z"))).resolves.toBe(1);
    await expect(closeElapsedPricingMonths(database, new Date("2026-07-02T00:00:00.000Z"))).resolves.toBe(0);
    await expect(getPricingView(database, user.id)).resolves.toMatchObject({
      tier: "studio", discountPercent: 75, multiplierBp: 2500,
    });
  });

  it("binds a one-time B2B invitation to email and supports manual pricing", async () => {
    const tokenHash = sha256("business-invite-token");
    await createBusinessInvite(database, {
      email: "founder@example.com",
      tokenHash,
      multiplierBp: 1500,
      expiresAt: new Date(Date.now() + 86_400_000),
    });

    await expect(createEmailUser(
      database, "wrong@example.com", "test-password-hash", tokenHash,
    )).rejects.toBeInstanceOf(InvalidBusinessInvitationError);

    const user = await createEmailUser(
      database, "founder@example.com", "test-password-hash", tokenHash,
    );
    await completeEngineAccount(database, user.id, "acct_business");
    expect(user).toMatchObject({ customerType: "b2b", engineMultiplierBp: 1500 });
    await expect(getPricingView(database, user.id)).resolves.toMatchObject({
      customerType: "b2b", pricingMode: "manual", discountPercent: 85, multiplierBp: 1500,
    });
    await expect(createEmailUser(
      database, "founder@example.com", "test-password-hash", tokenHash,
    )).rejects.toBeInstanceOf(InvalidBusinessInvitationError);

    await setBusinessPricing(database, { userId: user.id, multiplierBp: 1250, actorId: "test-admin" });
    await expect(getPricingView(database, user.id)).resolves.toMatchObject({
      discountPercent: 87.5, multiplierBp: 1250,
    });
    const job = await database.pool.query(`
      SELECT multiplier_bp, reason, status FROM engine_pricing_jobs WHERE user_id = $1
    `, [user.id]);
    expect(job.rows).toEqual([{ multiplier_bp: 1250, reason: "b2b_manual", status: "pending" }]);
  });

  async function clean(): Promise<void> {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities, checkout_sessions, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
  }
});

function ledgerCharge(id: string, amountNano: bigint) {
  return {
    id,
    kind: "charge" as const,
    amount_nano: amountNano.toString(),
    amount: `$${amountNano / 1_000_000_000n}.000000000`,
    key_masked: "sk-pool-test…test",
    ref: null,
    balance_after_nano: "0",
    ts: Math.floor(Date.now() / 1000).toString(),
  };
}

function sha256(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}
