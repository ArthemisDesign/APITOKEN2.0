import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { B2C_SIGNUP_BONUS_BALANCE_NANO } from "@claude-api/contracts";
import { createDatabase, type Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { settleSignupBonus } from "./signup-bonus.js";

const connectionString = process.env.TEST_DATABASE_URL;
const DEVICE_TOKEN = "a".repeat(43);

describe.runIf(Boolean(connectionString))("settleSignupBonus", () => {
  let database: Database;
  let creditAccount: ReturnType<typeof vi.fn>;
  let engine: EngineClient;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE signup_profiles, device_sightings, engine_accounts, customer_profiles,
               auth_identities, users RESTART IDENTITY CASCADE
    `);
    creditAccount = vi.fn(async (account: string) => ({
      account, balance_nano: "5000000000", balance: "$5.000000000",
    }));
    engine = { creditAccount } as unknown as EngineClient;
  });

  afterAll(async () => {
    await database.pool.query(`
      TRUNCATE signup_profiles, device_sightings, engine_accounts, customer_profiles,
               auth_identities, users RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  it("records the profile while the engine account is pending and claims once it turns active", async () => {
    const userId = await createUser(database, "pending-user@gmail.com", "pending", null);

    await settleSignupBonus(database, engine, {
      userId,
      email: "pending-user@gmail.com",
      customerType: "b2c",
      meta: { userAgent: "browser", ipAddress: "203.0.113.7", deviceToken: DEVICE_TOKEN },
    });

    expect(creditAccount).not.toHaveBeenCalled();
    const pending = await database.pool.query(`
      SELECT email_canonical, ip_subnet, user_agent, device_hash, bonus_granted, flagged_reason
      FROM signup_profiles WHERE user_id = $1
    `, [userId]);
    expect(pending.rows[0]).toMatchObject({
      email_canonical: "pending-user@gmail.com",
      ip_subnet: "203.0.113.0/24",
      user_agent: "browser",
      bonus_granted: false,
      flagged_reason: null,
    });
    expect(pending.rows[0].device_hash).toBeTruthy();

    // Worker подтвердил managed-политику → аккаунт активен; повторный вызов клеймит бонус.
    await database.pool.query(`
      UPDATE engine_accounts SET status = 'active', engine_account_id = 'acct_pending_user'
      WHERE user_id = $1
    `, [userId]);
    await settleSignupBonus(database, engine, {
      userId, email: "pending-user@gmail.com", customerType: "b2c",
    });

    expect(creditAccount).toHaveBeenCalledOnce();
    expect(creditAccount).toHaveBeenCalledWith(
      "acct_pending_user", B2C_SIGNUP_BONUS_BALANCE_NANO, `signup-bonus:${userId}`,
    );
    const granted = await database.pool.query(`
      SELECT bonus_granted, bonus_amount_nano::text AS amount FROM signup_profiles WHERE user_id = $1
    `, [userId]);
    expect(granted.rows[0]).toEqual({ bonus_granted: true, amount: "5000000000" });
  });

  it("self-heals an active account without any profile using null signals", async () => {
    const userId = await createUser(database, "legacy-oauth@gmail.com", "active", "acct_legacy");

    await settleSignupBonus(database, engine, {
      userId, email: "legacy-oauth@gmail.com", customerType: "b2c",
    });

    expect(creditAccount).toHaveBeenCalledWith(
      "acct_legacy", B2C_SIGNUP_BONUS_BALANCE_NANO, `signup-bonus:${userId}`,
    );
    const profile = await database.pool.query(`
      SELECT ip_address, ip_subnet, device_hash, bonus_granted FROM signup_profiles WHERE user_id = $1
    `, [userId]);
    expect(profile.rows[0]).toEqual({
      ip_address: null, ip_subnet: null, device_hash: null, bonus_granted: true,
    });
  });

  it("flags ineligible email domains instead of granting", async () => {
    const userId = await createUser(database, "abuser@protonmail.com", "active", "acct_proton");

    await settleSignupBonus(database, engine, {
      userId, email: "abuser@protonmail.com", customerType: "b2c",
    });

    expect(creditAccount).not.toHaveBeenCalled();
    const profile = await database.pool.query(
      "SELECT flagged_reason FROM signup_profiles WHERE user_id = $1", [userId],
    );
    expect(profile.rows[0]?.flagged_reason).toBe("email-domain");
  });

  it("flags a signup wave from the same /24 subnet", async () => {
    for (let index = 0; index < 4; index += 1) {
      const earlierId = await createUser(database, `wave${index}@gmail.com`, "pending", null);
      await database.pool.query(`
        INSERT INTO signup_profiles (user_id, email_canonical, ip_address, ip_subnet)
        VALUES ($1, $2, $3, '198.51.100.0/24')
      `, [earlierId, `wave${index}@gmail.com`, `198.51.100.${index + 1}`]);
    }
    const userId = await createUser(database, "wave-late@gmail.com", "active", "acct_wave");

    await settleSignupBonus(database, engine, {
      userId,
      email: "wave-late@gmail.com",
      customerType: "b2c",
      meta: { userAgent: null, ipAddress: "198.51.100.200", deviceToken: null },
    });

    expect(creditAccount).not.toHaveBeenCalled();
    const profile = await database.pool.query(
      "SELECT flagged_reason FROM signup_profiles WHERE user_id = $1", [userId],
    );
    expect(profile.rows[0]?.flagged_reason).toBe("subnet-velocity");
  });

  it("flags the second account sharing a granted device", async () => {
    const firstId = await createUser(database, "first-device@gmail.com", "active", "acct_first");
    await settleSignupBonus(database, engine, {
      userId: firstId,
      email: "first-device@gmail.com",
      customerType: "b2c",
      meta: { userAgent: null, ipAddress: "192.0.2.10", deviceToken: DEVICE_TOKEN },
    });
    expect(creditAccount).toHaveBeenCalledOnce();

    const secondId = await createUser(database, "second-device@gmail.com", "active", "acct_second");
    await settleSignupBonus(database, engine, {
      userId: secondId,
      email: "second-device@gmail.com",
      customerType: "b2c",
      meta: { userAgent: null, ipAddress: "192.0.2.11", deviceToken: DEVICE_TOKEN },
    });

    expect(creditAccount).toHaveBeenCalledOnce();
    const profile = await database.pool.query(
      "SELECT bonus_granted, flagged_reason FROM signup_profiles WHERE user_id = $1", [secondId],
    );
    expect(profile.rows[0]).toEqual({ bonus_granted: false, flagged_reason: "duplicate-device" });
  });

  it("releases the claim when the engine credit fails and grants on retry", async () => {
    const userId = await createUser(database, "flaky@gmail.com", "active", "acct_flaky");
    creditAccount.mockRejectedValueOnce(new Error("engine unavailable"));

    await expect(settleSignupBonus(database, engine, {
      userId, email: "flaky@gmail.com", customerType: "b2c",
    })).rejects.toThrow("engine unavailable");
    const released = await database.pool.query(
      "SELECT bonus_granted, bonus_amount_nano FROM signup_profiles WHERE user_id = $1", [userId],
    );
    expect(released.rows[0]).toEqual({ bonus_granted: false, bonus_amount_nano: null });

    await settleSignupBonus(database, engine, {
      userId, email: "flaky@gmail.com", customerType: "b2c",
    });
    expect(creditAccount).toHaveBeenCalledTimes(2);
    const granted = await database.pool.query(
      "SELECT bonus_granted FROM signup_profiles WHERE user_id = $1", [userId],
    );
    expect(granted.rows[0]?.bonus_granted).toBe(true);
  });

  it("does nothing for B2B accounts and never writes a profile", async () => {
    const userId = await createUser(database, "business@example.com", "active", "acct_b2b", "b2b");

    await settleSignupBonus(database, engine, {
      userId, email: "business@example.com", customerType: "b2b",
    });

    expect(creditAccount).not.toHaveBeenCalled();
    const profile = await database.pool.query(
      "SELECT count(*)::int AS n FROM signup_profiles WHERE user_id = $1", [userId],
    );
    expect(profile.rows[0]?.n).toBe(0);
  });

  it("is a no-op once the bonus is granted", async () => {
    const userId = await createUser(database, "twice@gmail.com", "active", "acct_twice");
    await settleSignupBonus(database, engine, {
      userId, email: "twice@gmail.com", customerType: "b2c",
    });
    expect(creditAccount).toHaveBeenCalledOnce();

    await settleSignupBonus(database, engine, {
      userId, email: "twice@gmail.com", customerType: "b2c",
    });
    expect(creditAccount).toHaveBeenCalledOnce();
  });
});

async function createUser(
  database: Database,
  email: string,
  status: "pending" | "active",
  engineAccountId: string | null,
  customerType: "b2c" | "b2b" = "b2c",
): Promise<string> {
  const userId = randomUUID();
  await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)", [
    userId, email, "Signup Bonus Test",
  ]);
  await database.pool.query(`
    INSERT INTO engine_accounts (id, user_id, engine_account_id, status)
    VALUES ($1, $2, $3, $4)
  `, [randomUUID(), userId, engineAccountId, status]);
  await database.pool.query(`
    INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
    VALUES ($1, $2, $3, 4000, date_trunc('month', now())::date)
  `, [userId, customerType, customerType === "b2b" ? null : 0]);
  return userId;
}
