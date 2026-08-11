import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { listAdminPayingUsers, listAdminRefunds } from "./admin-finance.js";

const connectionString = process.env.TEST_DATABASE_URL;

type FixtureUser =
  | "bonus"
  | "paid"
  | "manual"
  | "mixed"
  | "other"
  | "legacy"
  | "unattributed"
  | "bonus_topup";

describe.runIf(Boolean(connectionString))("paying users funding cohorts", () => {
  let database: Database;
  const users = new Map<FixtureUser, string>();
  let ledgerId = 0;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE pricing_usage_events, pricing_usage_topups,
               payments, checkout_sessions, users RESTART IDENTITY CASCADE
    `);
    users.clear();
    ledgerId = 0;
    for (const kind of [
      "bonus", "paid", "manual", "mixed", "other", "legacy", "unattributed", "bonus_topup",
    ] as const) {
      const userId = randomUUID();
      users.set(kind, userId);
      await database.pool.query(
        "INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)",
        [userId, `${kind}@test.invalid`, kind],
      );
      await database.pool.query(`
        INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
        VALUES ($1, $2, $3, 5000, 'active')
      `, [randomUUID(), userId, `acct-${userId}`]);
    }

    await insertPayment(users.get("paid")!);
    await insertManualTopup(users.get("manual")!);
    await insertBonusTopup(users.get("bonus_topup")!);

    await insertEvent(users.get("bonus")!, {
      amount: 100n, paid: 0n, eventProvider: "openai",
    });
    await insertEvent(users.get("paid")!, {
      amount: 200n, paid: 200n, eventProvider: "google",
    });
    await insertEvent(users.get("paid")!, {
      amount: 50n, paid: 50n, eventProvider: "google",
      engineAccountId: `acct-historical-${users.get("paid")!}`,
    });
    await insertEvent(users.get("manual")!, {
      amount: 300n, paid: 300n, eventProvider: "anthropic",
    });
    await insertEvent(users.get("mixed")!, {
      amount: 400n, paid: 100n, eventProvider: "anthropic",
    });
    await insertEvent(users.get("other")!, {
      amount: 500n, paid: 100n, eventProvider: "anthropic",
    });
    await insertEvent(users.get("legacy")!, {
      amount: 600n, paid: 600n, eventProvider: "google",
    });
    await insertEvent(users.get("unattributed")!, {
      amount: 700n, paid: 700n, eventProvider: null,
    });
  });

  afterAll(async () => {
    await database.pool.end();
  });

  async function insertPayment(userId: string): Promise<void> {
    const checkoutId = randomUUID();
    const providerPaymentId = `payment-${userId}`;
    await database.pool.query(`
      INSERT INTO checkout_sessions (
        id, user_id, engine_account_id, provider, amount_usd, amount_nano,
        provider_payment_id, status, provider_state, completed_at
      ) VALUES ($1, $2, $3, 'test', 25, 25000000000, $4, 'paid', '{}'::jsonb, now())
    `, [checkoutId, userId, `acct-${userId}`, providerPaymentId]);
    await database.pool.query(`
      INSERT INTO payments (
        id, checkout_id, user_id, provider, provider_payment_id, amount_minor,
        currency, amount_nano, status, provider_state, paid_at
      ) VALUES ($1, $2, $3, 'test', $4, 2500, 'USD', 25000000000,
                'paid', '{}'::jsonb, now())
    `, [randomUUID(), checkoutId, userId, providerPaymentId]);
  }

  async function insertManualTopup(userId: string): Promise<void> {
    await insertTopup(userId, "manual");
  }

  async function insertBonusTopup(userId: string): Promise<void> {
    await insertTopup(userId, "bonus");
  }

  async function insertTopup(userId: string, source: "manual" | "bonus"): Promise<void> {
    ledgerId += 1;
    await database.pool.query(`
      INSERT INTO pricing_usage_topups (
        id, user_id, engine_account_id, ledger_entry_id, ref, source, amount_nano, occurred_at
      ) VALUES ($1, $2, $3, $4, $5, $6, 1000, now())
    `, [randomUUID(), userId, `acct-${userId}`, ledgerId, `${source}:${userId}`, source]);
  }

  async function insertEvent(userId: string, input: {
    amount: bigint;
    eventProvider: string | null;
    /** The part of the charge covered by the customer's own money (free-first). */
    paid?: bigint;
    engineAccountId?: string;
  }): Promise<void> {
    ledgerId += 1;
    await database.pool.query(`
      INSERT INTO pricing_usage_events (
        id, user_id, engine_account_id, ledger_entry_id, provider_id,
        amount_nano, real_funded_nano, occurred_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, now())
    `, [
      randomUUID(),
      userId,
      input.engineAccountId ?? `acct-${userId}`,
      ledgerId,
      input.eventProvider,
      input.amount.toString(),
      (input.paid ?? 0n).toString(),
    ]);
  }

  it("includes only fully bonus-funded spend and keeps the engine provider split", async () => {
    const page = await listAdminPayingUsers(database, { days: 30, funding: "bonus" });
    expect(page.total).toBe(1);
    expect(page.rows).toEqual([
      expect.objectContaining({
        userId: users.get("bonus"),
        fundingKind: "bonus_only",
        paidNano: "0",
        paymentsCount: 0,
        manualPaidNano: "0",
        manualTopupsCount: 0,
        lastPaidAt: null,
        spentNano: "100",
        paidFundedSpentNano: "0",
        bonusFundedSpentNano: "100",
        otherFundedSpentNano: "0",
        unattributedSpentNano: "0",
        providerSpendNano: { anthropic: "0", openai: "100", google: "0", kimi: "0", other: "0" },
        engineAccountId: `acct-${users.get("bonus")}`,
      }),
    ]);
    expect(page.summary).toMatchObject({
      payingUsers: 0,
      cohortUsers: 1,
      bonusOnlyUsers: 1,
      bonusOnlySpentNano: "100",
    });
  });

  it("unions the money cohort with bonus-only for all and preserves omitted funding", async () => {
    const omitted = await listAdminPayingUsers(database, { days: 30 });
    expect(new Set(omitted.rows.map((row) => row.userId))).toEqual(
      new Set([users.get("paid"), users.get("manual")]),
    );
    expect(omitted.summary).toMatchObject({ payingUsers: 2, cohortUsers: 2, bonusOnlyUsers: 0 });
    expect(omitted.rows.find((row) => row.userId === users.get("paid"))).toMatchObject({
      spentNano: "250",
      paidFundedSpentNano: "250",
      unattributedSpentNano: "0",
      engineAccountId: `acct-${users.get("paid")}`,
      usageAccountIds: [
        `acct-${users.get("paid")}`,
        `acct-historical-${users.get("paid")}`,
      ],
    });

    const all = await listAdminPayingUsers(database, { days: 30, funding: "all" });
    expect(new Set(all.rows.map((row) => row.userId))).toEqual(
      new Set([users.get("bonus"), users.get("paid"), users.get("manual")]),
    );
    expect(all.summary).toMatchObject({
      payingUsers: 2,
      cohortUsers: 3,
      bonusOnlyUsers: 1,
      bonusOnlySpentNano: "100",
    });
  });

  it("preserves the legacy payments and manual filters", async () => {
    const payments = await listAdminPayingUsers(database, { days: 30, funding: "payments" });
    expect(payments.rows.map((row) => row.userId)).toEqual([users.get("paid")]);
    expect(payments.rows[0]?.fundingKind).toBe("payments");

    const manual = await listAdminPayingUsers(database, { days: 30, funding: "manual" });
    expect(manual.rows.map((row) => row.userId)).toEqual([users.get("manual")]);
    expect(manual.rows[0]?.fundingKind).toBe("manual");
  });

  it("excludes paid/manual users, mixed/other, legacy/unattributed, and bonus topup without charge", async () => {
    const bonus = await listAdminPayingUsers(database, { days: 30, funding: "bonus" });
    const ids = new Set(bonus.rows.map((row) => row.userId));
    for (const kind of [
      "paid", "manual", "mixed", "other", "legacy", "unattributed", "bonus_topup",
    ] as const) {
      expect(ids.has(users.get(kind)!)).toBe(false);
    }
  });

  it("includes every positive spender, keeps strict bonus_only, and marks other zero-money spenders spend_only", async () => {
    const spenders = await listAdminPayingUsers(database, { days: 30, funding: "spenders" });
    expect(new Set(spenders.rows.map((row) => row.userId))).toEqual(new Set([
      users.get("bonus"), users.get("paid"), users.get("manual"), users.get("mixed"),
      users.get("other"), users.get("legacy"), users.get("unattributed"),
    ]));
    expect(spenders.rows.find((row) => row.userId === users.get("bonus"))?.fundingKind).toBe("bonus_only");
    for (const kind of ["mixed", "other", "legacy", "unattributed"] as const) {
      expect(spenders.rows.find((row) => row.userId === users.get(kind))?.fundingKind).toBe("spend_only");
    }
    expect(spenders.rows.some((row) => row.userId === users.get("bonus_topup"))).toBe(false);
    expect(spenders.summary).toMatchObject({
      payingUsers: 2,
      cohortUsers: 7,
      bonusOnlyUsers: 1,
      activeSpenders: 7,
    });
  });

  it("finds a spender by exact email regardless of incomplete attribution", async () => {
    const email = "wwwvatroke@gmail.com";
    await database.pool.query("UPDATE users SET email = $1 WHERE id = $2", [
      email,
      users.get("unattributed"),
    ]);

    const page = await listAdminPayingUsers(database, {
      days: 30,
      funding: "spenders",
      q: email,
    });

    expect(page.total).toBe(1);
    expect(page.rows).toEqual([
      expect.objectContaining({
        userId: users.get("unattributed"),
        email,
        fundingKind: "spend_only",
        spentNano: "700",
        // Free-first records an exact split on every event, so nothing is left unattributed.
        unattributedSpentNano: "0",
      }),
    ]);
  });

  it("filters by the provider the engine recorded on the charge", async () => {
    // The retired attribution carried a second provider id that could disagree with the ledger
    // row; there is one provider per charge now, and it is the engine's own column.
    const anthropic = await listAdminPayingUsers(database, {
      days: 30, funding: "all", provider: "anthropic",
    });
    expect(new Set(anthropic.rows.map((row) => row.userId))).toEqual(new Set([users.get("manual")]));

    const openai = await listAdminPayingUsers(database, {
      days: 30, funding: "all", provider: "openai",
    });
    expect(new Set(openai.rows.map((row) => row.userId))).toEqual(new Set([users.get("bonus")]));

    const google = await listAdminPayingUsers(database, {
      days: 30, funding: "all", provider: "google",
    });
    expect(new Set(google.rows.map((row) => row.userId))).toEqual(new Set([users.get("paid")]));

    // The KIMI predicate is new. With no KIMI events in the window it must select nobody — a
    // filter that silently matched everything would be indistinguishable from "no filter" here.
    const kimi = await listAdminPayingUsers(database, {
      days: 30, funding: "all", provider: "kimi",
    });
    expect(kimi.rows).toEqual([]);
  });

  it("shows the durable engine compensation beside a refunded payment", async () => {
    const userId = users.get("paid")!;
    const payment = await database.pool.query<{ id: string; amount_nano: string }>(`
      UPDATE payments
      SET status = 'refunded', updated_at = now()
      WHERE user_id = $1
      RETURNING id, amount_nano
    `, [userId]);
    const paymentRow = payment.rows[0]!;
    const webhookEventId = randomUUID();
    await database.pool.query(`
      INSERT INTO webhook_events (
        id, provider, provider_event_id, event_type, payload, status, attempts, processed_at
      ) VALUES ($1, 'test', $2, 'payment.refunded', '{}'::jsonb, 'processed', 1, now())
    `, [webhookEventId, `refund-${paymentRow.id}`]);
    await database.pool.query(`
      INSERT INTO engine_adjustments (
        id, payment_id, webhook_event_id, engine_account_id, kind, amount_nano,
        idempotency_ref, status, attempts, last_error
      ) VALUES ($1, $2, $3, $4, 'refund', $5, $6, 'retry', 2, 'engine unavailable')
    `, [
      randomUUID(),
      paymentRow.id,
      webhookEventId,
      `acct-${userId}`,
      (-BigInt(paymentRow.amount_nano)).toString(),
      `refund:${paymentRow.id}`,
    ]);

    const refunds = await listAdminRefunds(database, 25, 0);
    expect(refunds).toMatchObject({ total: 1, totalNano: paymentRow.amount_nano });
    expect(refunds.rows).toEqual([
      expect.objectContaining({
        id: paymentRow.id,
        userId,
        status: "refunded",
        adjustmentStatus: "retry",
        adjustmentConfirmedAt: null,
        adjustmentLastError: "engine unavailable",
      }),
    ]);
  });
});
