import { randomUUID } from "node:crypto";
import { ConfigService } from "@nestjs/config";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createDatabase, type Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { PromoService } from "./promo.service.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("promo referral durability", () => {
  let database: Database;
  let userId: string;
  let engineAccountId: string;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    userId = randomUUID();
    engineAccountId = `acct-promo-${userId}`;
    await database.pool.query(
      "TRUNCATE referral_attributions, engine_accounts, users RESTART IDENTITY CASCADE",
    );
    await database.pool.query(`
      INSERT INTO users (id, email, display_name)
      VALUES ($1, $2, 'Promo Referral Test')
    `, [userId, `${userId}@test.invalid`]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 5000, 'active')
    `, [randomUUID(), userId, engineAccountId]);
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({
      valueNano: "5000000000",
      partnerId: randomUUID(),
      referralCode: "partner-code",
      redemptionRef: `promo:${randomUUID()}`,
      discountBps: 0,
      pricingAffected: false,
      alreadyRedeemed: false,
    })));
  });

  afterEach(async () => {
    vi.unstubAllGlobals();
    await database.pool.query(`
      DROP TRIGGER IF EXISTS test_reject_promo_attribution ON referral_attributions;
      DROP FUNCTION IF EXISTS test_reject_promo_attribution();
    `);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  function service(engine: EngineClient): PromoService {
    return new PromoService(
      database,
      engine,
      new ConfigService<Environment, true>({
        SALES_API_URL: "http://sales.test",
        SALES_CONTROL_KEY: "test-control-key",
      } as Environment),
    );
  }

  it("commits partner attribution before issuing the idempotent engine credit", async () => {
    const creditAccount = vi.fn(async (account: string) => {
      const attribution = await database.pool.query<{ code: string }>(
        "SELECT code FROM referral_attributions WHERE user_id = $1",
        [userId],
      );
      expect(attribution.rows).toEqual([{ code: "partner-code" }]);
      return { account, balance_nano: "5000000000", balance: "$5.000000000" };
    });

    await expect(service({ creditAccount } as unknown as EngineClient).redeem(userId, "PROMO"))
      .resolves.toMatchObject({ credited_nano: "5000000000" });
    expect(creditAccount).toHaveBeenCalledOnce();
  });

  it("does not consume the sales code until the engine account is active", async () => {
    await database.pool.query(
      "UPDATE engine_accounts SET status = 'pending' WHERE user_id = $1",
      [userId],
    );
    const creditAccount = vi.fn();

    await expect(service({ creditAccount } as unknown as EngineClient).redeem(userId, "PROMO"))
      .rejects.toThrow("account is not ready");
    expect(fetch).not.toHaveBeenCalled();
    expect(creditAccount).not.toHaveBeenCalled();
  });

  it("fails before credit when attribution storage is down and succeeds on replay", async () => {
    await database.pool.query(`
      CREATE OR REPLACE FUNCTION test_reject_promo_attribution() RETURNS trigger AS $$
      BEGIN
        RAISE EXCEPTION 'forced promo attribution failure';
      END;
      $$ LANGUAGE plpgsql;
      CREATE TRIGGER test_reject_promo_attribution
      BEFORE INSERT ON referral_attributions
      FOR EACH ROW EXECUTE FUNCTION test_reject_promo_attribution();
    `);
    const creditAccount = vi.fn();

    await expect(service({ creditAccount } as unknown as EngineClient).redeem(userId, "PROMO"))
      .rejects.toThrow("could not preserve partner attribution");
    expect(creditAccount).not.toHaveBeenCalled();

    await database.pool.query(`
      DROP TRIGGER test_reject_promo_attribution ON referral_attributions;
      DROP FUNCTION test_reject_promo_attribution();
    `);
    creditAccount.mockResolvedValue({
      account: engineAccountId,
      balance_nano: "5000000000",
      balance: "$5.000000000",
    });
    await expect(service({ creditAccount } as unknown as EngineClient).redeem(userId, "PROMO"))
      .resolves.toMatchObject({ credited_nano: "5000000000" });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(creditAccount).toHaveBeenCalledOnce();
  });
});
