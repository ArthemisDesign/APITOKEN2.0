import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { EngineLedgerEntry } from "@claude-api/contracts";
import { createDatabase, type Database } from "./client.js";
import { applyPricingLedgerPage } from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

// Проверяем «бесплатное тратится первым» на живой схеме: классификацию источника денег (F1) и то,
// что страница леджера обрабатывается в хронологическом порядке независимо от порядка в массиве (F4).
describe.runIf(Boolean(connectionString))("applyPricingLedgerPage free-first accounting", () => {
  let db: Database;
  let userId: string;
  const engineAccountId = "acct_free_first";

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
    userId = randomUUID();
    await db.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'FF')", [userId, `${userId}@t.invalid`]);
    await db.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status) VALUES ($1, $2, $3, 'active')",
      [randomUUID(), userId, engineAccountId],
    );
    await db.pool.query(
      "INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start, free_balance_nano) VALUES ($1, 'b2c', 0, 4000, date_trunc('month', now()), 0)",
      [userId],
    );
    await db.pool.query(
      "INSERT INTO pricing_usage_cursors (engine_account_id, user_id, last_ledger_id) VALUES ($1, $2, 0)",
      [engineAccountId, userId],
    );
  });

  function entry(id: number, kind: "topup" | "charge" | "adjust", amountNano: bigint, ref: string | null): EngineLedgerEntry {
    return {
      id, kind, amount_nano: amountNano.toString(), amount: amountNano.toString(),
      key_masked: null, ref, balance_after_nano: null, ts: 1_700_000_000 + id, model: null,
    };
  }
  async function realFundedTotal(): Promise<bigint> {
    const r = await db.pool.query<{ t: string }>("SELECT COALESCE(SUM(real_funded_nano),0)::text AS t FROM pricing_usage_events WHERE user_id = $1", [userId]);
    return BigInt(r.rows[0]!.t);
  }
  async function freeBalance(): Promise<bigint> {
    const r = await db.pool.query<{ f: string }>("SELECT free_balance_nano::text AS f FROM customer_profiles WHERE user_id = $1", [userId]);
    return BigInt(r.rows[0]!.f);
  }

  it("F1: admin-credit funds the FREE bucket, so it is spent before real money and earns no commission", async () => {
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 100n, `admin-credit:${randomUUID()}`), // FREE (gift) — must not count as real
      entry(2, "topup", 50n, `platega:${randomUUID()}`),       // real deposit (not tracked in free bucket)
      entry(3, "charge", 120n, null),                           // spend 120: 100 free + 20 real
    ]);
    expect((await realFundedTotal())).toBe(20n); // only the real part is commissionable
    expect((await freeBalance())).toBe(0n);       // 100 free fully consumed
  });

  it("F4: an out-of-order page is processed chronologically (charge cannot precede its funding topup)", async () => {
    // Массив специально в обратном порядке: charge (id=3) стоит ПЕРЕД фондирующим бонусом (id=2).
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(3, "charge", 80n, null),
      entry(2, "topup", 80n, `signup-bonus:${userId}`),
    ]);
    // При корректной сортировке бонус фондирует charge → real_funded = 0. Без сортировки было бы 80.
    expect((await realFundedTotal())).toBe(0n);
  });

  it("negative adjust reversals are ignored here and never inflate real_funded (safe direction)", async () => {
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 100n, `platega:${randomUUID()}`),
      entry(2, "charge", 60n, null),                    // no free balance → real_funded 60
      entry(3, "adjust", -40n, `platega:${randomUUID()}`), // refund/clawback — not applied to commission
    ]);
    expect((await realFundedTotal())).toBe(60n); // unchanged by the adjust; never over-counts
  });
});
