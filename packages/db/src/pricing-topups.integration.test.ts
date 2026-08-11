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
  PricingLedgerEvidenceError,
} from "./pricing.js";
import { listUsageEventsAfter } from "./sales-feed.js";

const connectionString = process.env.TEST_DATABASE_URL;

// Пополнения, сделанные напрямую в движке (подарочные admin-credit и ручные внешние зачисления),
// не создают строк в payments. Ledger-копия сохраняет оба источника, но только подтверждённое
// ручное внешнее зачисление входит в money-funded cohort.
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

  function entry(
    id: number,
    kind: "topup" | "charge",
    amountNano: bigint,
    ref: string | null,
    uncollectedNano = 0n,
  ): EngineLedgerEntry {
    return {
      id: String(id), kind, amount_nano: amountNano.toString(), amount: amountNano.toString(),
      key_masked: null, ref, balance_after_nano: null, ts: String(1_700_000_000 + id), model: null,
      uncollected_nano: uncollectedNano.toString(),
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
    expect(classifyTopupRef("admin-credit:abc")).toBe("bonus");
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
      { source: "bonus", amount_nano: "1000" },
      { source: "bonus", amount_nano: "500" },
      { source: "payment", amount_nano: "250" },
    ]);
  });

  it("stores full billed spend but funds and commissions only the collected remainder", async () => {
    await db.pool.query(
      "UPDATE customer_profiles SET free_balance_nano = 60 WHERE user_id = $1",
      [userId],
    );
    await db.pool.query(
      "INSERT INTO referral_attributions (user_id, code, created_at) VALUES ($1, 'shortfall-partner', now() - interval '1 minute')",
      [userId],
    );
    const charge = entry(1, "charge", 100n, null, 30n);
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [charge]);
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [charge]);

    const event = await db.pool.query<{
      amount_nano: string;
      uncollected_nano: string;
      real_funded_nano: string;
    }>(`
      UPDATE pricing_usage_events
      SET created_at = now() - interval '1 minute'
      WHERE user_id = $1
      RETURNING amount_nano::text, uncollected_nano::text, real_funded_nano::text
    `, [userId]);
    expect(event.rows).toEqual([{
      amount_nano: "100",
      uncollected_nano: "30",
      real_funded_nano: "10",
    }]);
    const profile = await db.pool.query<{ free_balance_nano: string }>(
      "SELECT free_balance_nano::text FROM customer_profiles WHERE user_id = $1",
      [userId],
    );
    expect(profile.rows[0]?.free_balance_nano).toBe("0");

    const feed = await listUsageEventsAfter(db, 0n, 10);
    expect(feed.items).toEqual([
      expect.objectContaining({ userId, amountNano: 10n }),
    ]);
  });

  it("rejects contradictory shortfall evidence without moving funding or the cursor", async () => {
    await db.pool.query(
      "UPDATE customer_profiles SET free_balance_nano = 60 WHERE user_id = $1",
      [userId],
    );
    await expect(applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "charge", 100n, null, 101n),
    ])).rejects.toBeInstanceOf(PricingLedgerEvidenceError);
    await expect(applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 100n, "admin-credit:invalid", 1n),
    ])).rejects.toBeInstanceOf(PricingLedgerEvidenceError);

    const state = await db.pool.query<{
      free_balance_nano: string;
      last_ledger_id: string;
      event_count: string;
      topup_count: string;
    }>(`
      SELECT p.free_balance_nano::text,
             c.last_ledger_id::text,
             (SELECT count(*)::text FROM pricing_usage_events WHERE user_id = $1) AS event_count,
             (SELECT count(*)::text FROM pricing_usage_topups WHERE user_id = $1) AS topup_count
      FROM customer_profiles p
      JOIN pricing_usage_cursors c ON c.user_id = p.user_id
      WHERE p.user_id = $1
    `, [userId]);
    expect(state.rows[0]).toEqual({
      free_balance_nano: "60",
      last_ledger_id: "0",
      event_count: "0",
      topup_count: "0",
    });
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
    expect(await topups()).toEqual([{ source: "bonus", amount_nano: "700" }]);
    expect(await getPricingTopupBackfillCursor(db, { userId, engineAccountId }, 9n)).toBeNull();
  });

  it("«оплачено» = платежи + ручные внешние пополнения, без двойного счёта и подарков", async () => {
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
      entry(4, "topup", 1000n, "manual-balance-1000"),
      entry(5, "charge", 40n, null),
    ]);

    const page = await listAdminPayingUsers(db, { days: 30 });
    const row = page.rows.find((item) => item.userId === userId);
    expect(row).toBeDefined();
    // 250 платежа + 1000 ручного внешнего зачисления; admin-credit, бонус и топап-двойник
    // платежа не учитываются.
    expect(row!.paidNano).toBe("1250");
    expect(row!.manualPaidNano).toBe("1000");
    expect(row!.paymentsCount).toBe(1);
    expect(row!.manualTopupsCount).toBe(1);
    expect(page.summary.paidNano).toBe("1250");
    expect(page.summary.manualPaidNano).toBe("1000");
  });

  it("фильтр источника денег делит когорту и сужает сводку", async () => {
    // Клиент A: только ручное внешнее зачисление. Клиент B: подтверждённый платёж.
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 900n, "manual-balance-900"),
      entry(2, "charge", 90n, null),
    ]);
    const payerId = randomUUID();
    const payerAccount = "acct_payer";
    await db.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'P')", [
      payerId, `${payerId}@t.invalid`,
    ]);
    await db.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status) VALUES ($1, $2, $3, 'active')",
      [randomUUID(), payerId, payerAccount],
    );
    const checkoutId = randomUUID();
    await db.pool.query(
      `INSERT INTO checkout_sessions (id, user_id, engine_account_id, provider, amount_usd, amount_nano, status)
       VALUES ($1, $2, $3, 'platega', 1, 1000000000, 'paid')`,
      [checkoutId, payerId, payerAccount],
    );
    await db.pool.query(
      `INSERT INTO payments (id, user_id, provider, provider_payment_id, amount_minor, currency,
        amount_nano, status, paid_at, checkout_id)
       VALUES ($1, $2, 'platega', 'pay-real', 100, 'USD', 700, 'paid', now(), $3)`,
      [randomUUID(), payerId, checkoutId],
    );

    const all = await listAdminPayingUsers(db, { days: 30 });
    expect(all.summary.payingUsers).toBe(2);

    const payments = await listAdminPayingUsers(db, { days: 30, funding: "payments" });
    expect(payments.rows.map((row) => row.userId)).toEqual([payerId]);
    expect(payments.summary.payingUsers).toBe(1);
    expect(payments.summary.paidNano).toBe("700");
    expect(payments.summary.manualPaidNano).toBe("0");

    const manual = await listAdminPayingUsers(db, { days: 30, funding: "manual" });
    expect(manual.rows.map((row) => row.userId)).toEqual([userId]);
    expect(manual.summary.paidNano).toBe("900");
  });

  it("клиент без единого платежа, но с ручным пополнением, считается платящим", async () => {
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 5000n, "manual-balance-offline-deal"),
      entry(2, "charge", 300n, null),
    ]);
    const page = await listAdminPayingUsers(db, { days: 30 });
    expect(page.rows.map((row) => row.userId)).toContain(userId);
    expect(page.summary.payingUsers).toBe(1);
  });

  it("admin-credit остаётся подарком и не создаёт paying-user без внешнего платежа", async () => {
    const recentCharge = entry(2, "charge", 300n, null);
    recentCharge.ts = String(Math.floor(Date.now() / 1000) - 60);
    await applyPricingLedgerPage(db, { userId, engineAccountId }, [
      entry(1, "topup", 5000n, "admin-credit:gift"),
      recentCharge,
    ]);

    const paid = await listAdminPayingUsers(db, { days: 30 });
    expect(paid.rows.map((row) => row.userId)).not.toContain(userId);
    expect(paid.summary.payingUsers).toBe(0);

    const bonus = await listAdminPayingUsers(db, { days: 30, funding: "bonus" });
    expect(bonus.rows).toEqual([
      expect.objectContaining({
        userId,
        fundingKind: "bonus_only",
        paidNano: "0",
        manualPaidNano: "0",
        spentNano: "300",
        paidFundedSpentNano: "0",
        bonusFundedSpentNano: "300",
      }),
    ]);
  });
});
