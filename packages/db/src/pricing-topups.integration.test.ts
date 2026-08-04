import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { EngineLedgerEntry } from "@claude-api/contracts";
import { createDatabase, type Database } from "./client.js";
import { listAdminPayingUsers } from "./admin-finance.js";
import {
  applyPricingLedgerPage,
  applyPricingTopupBackfillPage,
  classifyTopupRef,
  getPricingTopupBackfillCursor,
} from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

// Пополнения, сделанные напрямую в движке (admin-credit, ручные зачисления), не создают строк в
// payments — до этой правки такой клиент вообще не считался платящим, и его расход (в т.ч. весь
// GPT/Gemini) не попадал ни в один финансовый отчёт админки.
describe.runIf(Boolean(connectionString))("engine top-ups recorded for reporting", () => {
  let db: Database;
  let userId: string;
  const engineAccountId = "acct_topups";

  beforeAll(async () => {
    db = createDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });
  afterAll(async () => {
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query(
      `TRUNCATE customer_profiles, pricing_usage_events, pricing_usage_topups, pricing_usage_cursors,
       pricing_months, engine_accounts, payments, checkout_sessions, users RESTART IDENTITY CASCADE`,
    );
    userId = randomUUID();
    await db.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'T')", [
      userId, `${userId}@t.invalid`,
    ]);
    await db.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status) VALUES ($1, $2, $3, 'active')",
      [randomUUID(), userId, engineAccountId],
    );
    await db.pool.query(
      `INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start, free_balance_nano)
       VALUES ($1, 'b2c', 0, 4000, date_trunc('month', now()), 0)`,
      [userId],
    );
    await db.pool.query(
      "INSERT INTO pricing_usage_cursors (engine_account_id, user_id, last_ledger_id) VALUES ($1, $2, 0)",
      [engineAccountId, userId],
    );
  });

  function entry(id: number, kind: "topup" | "charge", amountNano: bigint, ref: string | null): EngineLedgerEntry {
    return {
      id: String(id), kind, amount_nano: amountNano.toString(), amount: amountNano.toString(),
      key_masked: null, ref, balance_after_nano: null, ts: String(1_700_000_000 + id), model: null,
    };
  }
  async function topups(): Promise<Array<{ source: string; amount_nano: string }>> {
    const result = await db.pool.query<{ source: string; amount_nano: string }>(
      "SELECT source, amount_nano::text FROM pricing_usage_topups WHERE user_id = $1 ORDER BY ledger_entry_id",
      [userId],
    );
    return result.rows;
  }

  it("классифицирует источник денег и никогда не считает подарок оплатой", () => {
    expect(classifyTopupRef("platega:abc")).toBe("payment");
    expect(classifyTopupRef("cryptomus:abc")).toBe("payment");
    expect(classifyTopupRef("signup-bonus:u1")).toBe("bonus");
    expect(classifyTopupRef("promo:new-year")).toBe("bonus");
    expect(classifyTopupRef("admin-credit:abc")).toBe("manual");
    expect(classifyTopupRef("manual-balance-500")).toBe("manual");
    expect(classifyTopupRef(null)).toBe("manual");
  });

  it("сохраняет каждое пополнение ровно один раз при повторной подаче страницы", async () => {
    const page = [
      entry(1, "topup", 1000n, "admin-credit:one"),
      entry(2, "topup", 500n, `signup-bonus:${userId}`),
      entry(3, "topup", 250n, "platega:pay-1"),
      entry(4, "charge", 100n, null),
    ];
    await applyPricingLedgerPage(db, { userId, engineAccountId }, page);
    await applyPricingLedgerPage(db, { userId, engineAccountId }, page);
    expect(await topups()).toEqual([
      { source: "manual", amount_nano: "1000" },
      { source: "bonus", amount_nano: "500" },
      { source: "payment", amount_nano: "250" },
    ]);
  });

  it("догоняющий скан заполняет историю ниже курсора и останавливается", async () => {
    await db.pool.query(
      "UPDATE pricing_usage_cursors SET last_ledger_id = 9 WHERE engine_account_id = $1",
      [engineAccountId],
    );
    const cursor = await getPricingTopupBackfillCursor(db, { userId, engineAccountId }, 9n);
    expect(cursor).toBe(0n);
    const recorded = await applyPricingTopupBackfillPage(db, { userId, engineAccountId }, [
      entry(5, "topup", 700n, "admin-credit:history"),
      entry(6, "charge", 70n, null),
    ], 9n);
    expect(recorded).toBe(1);
    expect(await topups()).toEqual([{ source: "manual", amount_nano: "700" }]);
    expect(await getPricingTopupBackfillCursor(db, { userId, engineAccountId }, 9n)).toBeNull();
  });

  it("«оплачено» = платежи + ручные пополнения, без двойного счёта и без бонусов", async () => {
    const checkoutId = randomUUID();
    await db.pool.query(
      `INSERT INTO checkout_sessions (id, user_id, engine_account_id, provider, amount_usd, amount_nano, status)
       VALUES ($1, $2, $3, 'platega', 1, 1000000000, 'paid')`,
      [checkoutId, userId, engineAccountId],
    );
    await db.pool.query(
      `INSERT INTO payments (id, user_id, provider, provider_payment_id, amount_minor, currency,
        amount_nano, status, paid_at, checkout_id)
       VALUES ($1, $2, 'platega', 'pay-1', 100, 'USD', 250, 'paid', now(), $3)`,
      [randomUUID(), userId, checkoutId],
    );
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 1000n, "admin-credit:one"),
      entry(2, "topup", 500n, `signup-bonus:${userId}`),
      entry(3, "topup", 250n, "platega:pay-1"),
      entry(4, "charge", 40n, null),
    ]);

    const page = await listAdminPayingUsers(db, { days: 30 });
    const row = page.rows.find((item) => item.userId === userId);
    expect(row).toBeDefined();
    // 250 платежа + 1000 ручного зачисления; бонус 500 и топап-двойник платежа не учитываются.
    expect(row!.paidNano).toBe("1250");
    expect(row!.manualPaidNano).toBe("1000");
    expect(row!.paymentsCount).toBe(1);
    expect(row!.manualTopupsCount).toBe(1);
    expect(page.summary.paidNano).toBe("1250");
    expect(page.summary.manualPaidNano).toBe("1000");
  });

  it("клиент без единого платежа, но с ручным пополнением, считается платящим", async () => {
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 5000n, "admin-credit:offline-deal"),
      entry(2, "charge", 300n, null),
    ]);
    const page = await listAdminPayingUsers(db, { days: 30 });
    expect(page.rows.map((row) => row.userId)).toContain(userId);
    expect(page.summary.payingUsers).toBe(1);
  });
});
