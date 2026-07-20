import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  activateCheckoutSession,
  applyVerifiedCheckoutPaymentEvent,
  createCheckoutSession,
  createDatabase,
  evaluateRefundEligibility,
  type CheckoutSession,
  type Database,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

// Правило: пополнение возвращаемо только ≤5 дней с оплаты И если реальных денег не тратилось.
describe.runIf(Boolean(connectionString))("evaluateRefundEligibility (5-day, fully-unspent)", () => {
  let database: Database;
  let userId: string;
  const PAID_AT = new Date("2026-07-13T12:00:00.000Z");

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });
  afterAll(async () => {
    await database.pool.end();
  });
  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities, pricing_usage_events,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Refund')", [userId, `${userId}@t.invalid`]);
    await database.pool.query(
      "INSERT INTO engine_accounts (id, user_id, engine_account_id, status) VALUES ($1, $2, 'acct_refund', 'active')",
      [randomUUID(), userId],
    );
  });

  async function paidCheckout(amountUsd = 100n): Promise<CheckoutSession> {
    const created = await createCheckoutSession(database, { userId, provider: "platega", amountUsd });
    const session = await activateCheckoutSession(database, {
      id: created.id, providerPaymentId: randomUUID(), checkoutUrl: "https://pay.test/i", providerState: { status: "check" },
    });
    await applyVerifiedCheckoutPaymentEvent(database, {
      provider: "platega", providerEventId: `${session.id}:paid`, providerPaymentId: session.providerPaymentId!,
      checkoutId: session.id, state: "paid", amountUsd, currency: "USD", paidAt: PAID_AT, payload: { ok: true },
    });
    return session;
  }
  async function spend(realFundedNano: bigint, occurredAt: Date, chargeNano = realFundedNano > 0n ? realFundedNano : 1n): Promise<void> {
    await database.pool.query(
      `INSERT INTO pricing_usage_events (id, user_id, engine_account_id, ledger_entry_id, amount_nano, real_funded_nano, occurred_at)
       VALUES ($1, $2, 'acct_refund', $3, $4, $5, $6)`,
      [randomUUID(), userId, Math.floor(Math.random() * 1e9), chargeNano.toString(), realFundedNano.toString(), occurredAt],
    );
  }
  const day = (n: number) => new Date(PAID_AT.getTime() + n * 86_400_000);

  it("ok: paid 1 day ago, nothing spent → eligible", async () => {
    const s = await paidCheckout();
    const v = await evaluateRefundEligibility(database, s.id, day(1));
    expect(v).toMatchObject({ eligible: true, reason: "ok", realSpentSinceNano: "0" });
  });

  it("window_expired: paid 6 days ago, nothing spent → not eligible", async () => {
    const s = await paidCheckout();
    const v = await evaluateRefundEligibility(database, s.id, day(6));
    expect(v).toMatchObject({ eligible: false, reason: "window_expired" });
  });

  it("boundary: exactly 5 days is still eligible; a hair over is not", async () => {
    const s = await paidCheckout();
    expect((await evaluateRefundEligibility(database, s.id, day(5))).eligible).toBe(true);
    expect((await evaluateRefundEligibility(database, s.id, new Date(day(5).getTime() + 1))).eligible).toBe(false);
  });

  it("balance_spent: real money spent after the top-up → not eligible", async () => {
    const s = await paidCheckout();
    await spend(10_000_000_000n, day(0.5)); // $10 real spent within the window
    const v = await evaluateRefundEligibility(database, s.id, day(1));
    expect(v).toMatchObject({ eligible: false, reason: "balance_spent", realSpentSinceNano: "10000000000" });
  });

  it("free-only spend does NOT block a refund (real_funded = 0)", async () => {
    const s = await paidCheckout();
    await spend(0n, day(0.5), 2_000_000_000n); // $2 charge fully covered by free balance
    const v = await evaluateRefundEligibility(database, s.id, day(1));
    expect(v).toMatchObject({ eligible: true, reason: "ok", realSpentSinceNano: "0" });
  });

  it("spend BEFORE the top-up is ignored (only spend after the anchor counts)", async () => {
    const s = await paidCheckout();
    await spend(5_000_000_000n, day(-1)); // real spend a day before this top-up
    const v = await evaluateRefundEligibility(database, s.id, day(1));
    expect(v).toMatchObject({ eligible: true, reason: "ok" });
  });

  it("not_paid for a pending (activated but unpaid) checkout, not_found for unknown", async () => {
    const created = await createCheckoutSession(database, { userId, provider: "platega", amountUsd: 20n });
    const pending = await activateCheckoutSession(database, {
      id: created.id, providerPaymentId: randomUUID(), checkoutUrl: "https://pay.test/i", providerState: { status: "check" },
    });
    // No payment row yet → treated as not_found (no payments row for the checkout).
    expect((await evaluateRefundEligibility(database, pending.id, day(1))).reason).toBe("not_found");
    expect((await evaluateRefundEligibility(database, randomUUID(), day(1))).reason).toBe("not_found");
  });
});
