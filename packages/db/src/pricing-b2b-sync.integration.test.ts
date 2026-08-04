import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { EngineLedgerEntry } from "@claude-api/contracts";
import { createDatabase, type Database } from "./client.js";
import { applyPricingLedgerPage, listPricingSyncTargets } from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

// B2B-клиент обязан синкаться так же, как b2c: его расход — источник правды для админки и
// провайдерской разбивки. Прогрессивная модель (free-first, месяцы, тир-окна) к нему НЕ
// применяется. До этой правки b2b выпадал из выборки целей и его курсор замирал навсегда.
describe.runIf(Boolean(connectionString))("pricing usage sync for b2b customers", () => {
  let db: Database;
  let b2bUserId: string;
  let b2cUserId: string;
  const b2bAccountId = "acct_b2b_sync";
  const b2cAccountId = "acct_b2c_sync";

  beforeAll(async () => {
    db = createDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });
  afterAll(async () => {
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query(
      "TRUNCATE customer_profiles, pricing_usage_events, pricing_usage_cursors, pricing_months, engine_accounts, users RESTART IDENTITY CASCADE",
    );
    b2bUserId = randomUUID();
    b2cUserId = randomUUID();
    await seedUser(b2bUserId, b2bAccountId, "b2b");
    await seedUser(b2cUserId, b2cAccountId, "b2c");
  });

  async function seedUser(userId: string, engineAccountId: string, type: "b2c" | "b2b"): Promise<void> {
    await db.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)", [
      userId,
      `${userId}@t.invalid`,
      type.toUpperCase(),
    ]);
    await db.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status) VALUES ($1, $2, $3, 'active')",
      [randomUUID(), userId, engineAccountId],
    );
    // У b2b нет тира: current_tier NULL — ровно как после конвертации админом.
    await db.pool.query(
      `INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start, free_balance_nano)
       VALUES ($1, $2, $3, 4000, date_trunc('month', now()), 0)`,
      [userId, type, type === "b2c" ? 0 : null],
    );
    await db.pool.query(
      "INSERT INTO pricing_usage_cursors (engine_account_id, user_id, last_ledger_id) VALUES ($1, $2, 0)",
      [engineAccountId, userId],
    );
  }

  function entry(id: number, kind: "topup" | "charge", amountNano: bigint, ref: string | null): EngineLedgerEntry {
    return {
      id: String(id), kind, amount_nano: amountNano.toString(), amount: amountNano.toString(),
      key_masked: null, ref, balance_after_nano: null, ts: String(1_700_000_000 + id), model: null,
    };
  }

  async function eventTotal(userId: string): Promise<bigint> {
    const result = await db.pool.query<{ total: string }>(
      "SELECT COALESCE(SUM(amount_nano), 0)::text AS total FROM pricing_usage_events WHERE user_id = $1",
      [userId],
    );
    return BigInt(result.rows[0]!.total);
  }

  it("b2b customers are pricing sync targets alongside b2c", async () => {
    const targets = await listPricingSyncTargets(db);
    expect(targets.map((target) => target.engineAccountId).sort()).toEqual([b2bAccountId, b2cAccountId].sort());
  });

  it("records b2b usage events and advances the cursor", async () => {
    await applyPricingLedgerPage(db, { userId: b2bUserId, engineAccountId: b2bAccountId }, [
      entry(11, "charge", 700n, null),
      entry(12, "charge", 300n, null),
    ]);
    expect(await eventTotal(b2bUserId)).toBe(1000n);
    const cursor = await db.pool.query<{ last_ledger_id: string }>(
      "SELECT last_ledger_id::text AS last_ledger_id FROM pricing_usage_cursors WHERE user_id = $1",
      [b2bUserId],
    );
    expect(cursor.rows[0]!.last_ledger_id).toBe("12");
  });

  it("never applies the progressive b2c projections to a b2b customer", async () => {
    await applyPricingLedgerPage(db, { userId: b2bUserId, engineAccountId: b2bAccountId }, [
      entry(21, "topup", 500n, `admin-credit:${randomUUID()}`),
      entry(22, "charge", 400n, null),
    ]);
    const profile = await db.pool.query<{ free_balance_nano: string; tier_window_spent_nano: string }>(
      "SELECT free_balance_nano::text AS free_balance_nano, tier_window_spent_nano::text AS tier_window_spent_nano FROM customer_profiles WHERE user_id = $1",
      [b2bUserId],
    );
    expect(profile.rows[0]!.free_balance_nano).toBe("0");
    expect(profile.rows[0]!.tier_window_spent_nano).toBe("0");
    const months = await db.pool.query("SELECT 1 FROM pricing_months WHERE user_id = $1", [b2bUserId]);
    expect(months.rowCount).toBe(0);
  });

  it("keeps a pre-attribution b2b charge out of the commission basis", async () => {
    // У b2b нет локальной free-first проекции, поэтому legacy-строка не создаёт базиса:
    // недоплатить безопасно, переплатить комиссию — нет.
    await applyPricingLedgerPage(db, { userId: b2bUserId, engineAccountId: b2bAccountId }, [
      entry(31, "topup", 900n, `platega:${randomUUID()}`),
      entry(32, "charge", 600n, null),
    ]);
    const funded = await db.pool.query<{ total: string }>(
      "SELECT COALESCE(SUM(real_funded_nano), 0)::text AS total FROM pricing_usage_events WHERE user_id = $1",
      [b2bUserId],
    );
    expect(funded.rows[0]!.total).toBe("0");
  });

  it("leaves b2c behaviour unchanged", async () => {
    await applyPricingLedgerPage(db, { userId: b2cUserId, engineAccountId: b2cAccountId }, [
      entry(41, "topup", 100n, `signup-bonus:${b2cUserId}`),
      entry(42, "charge", 250n, null),
    ]);
    const profile = await db.pool.query<{ free_balance_nano: string }>(
      "SELECT free_balance_nano::text AS free_balance_nano FROM customer_profiles WHERE user_id = $1",
      [b2cUserId],
    );
    expect(profile.rows[0]!.free_balance_nano).toBe("0"); // 100 бонуса списаны первыми
    const funded = await db.pool.query<{ total: string }>(
      "SELECT COALESCE(SUM(real_funded_nano), 0)::text AS total FROM pricing_usage_events WHERE user_id = $1",
      [b2cUserId],
    );
    expect(funded.rows[0]!.total).toBe("150");
    const months = await db.pool.query("SELECT 1 FROM pricing_months WHERE user_id = $1", [b2cUserId]);
    expect(months.rowCount).toBe(1);
  });
});
