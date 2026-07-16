import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  activateCheckoutSession,
  applyVerifiedCheckoutPaymentEvent,
  claimNextCredit,
  confirmCredit,
  createCheckoutSession,
  createDatabase,
  getCheckoutSession,
  type CheckoutSession,
  type Database,
  type VerifiedCheckoutPaymentEvent,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("whole-USD checkout persistence", () => {
  let database: Database;
  let userId: string;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)", [
      userId, `${userId}@test.invalid`, "Payment Test",
    ]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, status)
      VALUES ($1, $2, 'acct_integration', 'active')
    `, [randomUUID(), userId]);
  });

  afterAll(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  async function checkout(amountUsd = 25n): Promise<CheckoutSession> {
    const created = await createCheckoutSession(database, { userId, provider: "cryptomus", amountUsd });
    return activateCheckoutSession(database, {
      id: created.id,
      providerPaymentId: "26109ba0-b05b-4ee0-93d1-fd62c822ce95",
      checkoutUrl: "https://pay.test/invoice",
      providerState: { status: "check" },
    });
  }

  function event(session: CheckoutSession, overrides: Partial<VerifiedCheckoutPaymentEvent> = {}): VerifiedCheckoutPaymentEvent {
    return {
      provider: "cryptomus",
      providerEventId: "payment-1:paid",
      providerPaymentId: session.providerPaymentId!,
      checkoutId: session.id,
      state: "paid",
      amountUsd: session.amountUsd,
      currency: "USD",
      paidAt: new Date("2026-07-13T12:00:00Z"),
      payload: { safe: "fixture" },
      ...overrides,
    };
  }

  it("stores arbitrary whole USD as exact nanoUSD", async () => {
    const session = await checkout(37n);
    expect(session).toMatchObject({ amountUsd: 37n, amountNano: 37_000_000_000n, status: "pending" });
    await expect(getCheckoutSession(database, { id: session.id, userId })).resolves.toMatchObject({ amountUsd: 37n });
  });

  it("persists a verified payment and engine credit atomically", async () => {
    const session = await checkout();
    const accepted = await applyVerifiedCheckoutPaymentEvent(database, event(session));
    expect(accepted).toMatchObject({ duplicateEvent: false, checkoutStatus: "paid" });

    const payment = await database.pool.query("SELECT status, amount_minor, amount_nano FROM payments");
    const credit = await database.pool.query("SELECT status, idempotency_ref FROM engine_credits");
    expect(payment.rows).toEqual([{ status: "paid", amount_minor: "2500", amount_nano: "25000000000" }]);
    expect(credit.rows).toEqual([{ status: "pending", idempotency_ref: `cryptomus:${session.providerPaymentId}` }]);
  });

  it("deduplicates deliveries and never creates a second credit", async () => {
    const session = await checkout();
    const first = await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const duplicate = await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const paidOver = await applyVerifiedCheckoutPaymentEvent(database, event(session, { providerEventId: "payment-1:paid_over" }));

    expect(first.duplicateEvent).toBe(false);
    expect(duplicate).toEqual({ duplicateEvent: true, paymentId: null, creditId: null, checkoutStatus: null });
    expect(paidOver.paymentId).toBe(first.paymentId);
    expect(paidOver.creditId).toBe(first.creditId);
    const counts = await database.pool.query(`
      SELECT (SELECT count(*)::int FROM payments) AS payments,
             (SELECT count(*)::int FROM engine_credits) AS credits,
             (SELECT count(*)::int FROM webhook_events) AS events
    `);
    expect(counts.rows[0]).toEqual({ payments: 1, credits: 1, events: 2 });
  });

  it("rejects a verified amount different from the stored checkout", async () => {
    const session = await checkout();
    await expect(applyVerifiedCheckoutPaymentEvent(database, event(session, {
      providerEventId: "payment-1:mismatch",
      amountUsd: 99n,
    }))).rejects.toThrow("amount does not match");
    const events = await database.pool.query("SELECT count(*)::int AS count FROM webhook_events");
    expect(events.rows[0]).toEqual({ count: 0 });
  });

  it("records cancellation without issuing credit", async () => {
    const session = await checkout();
    const result = await applyVerifiedCheckoutPaymentEvent(database, event(session, {
      providerEventId: "payment-1:cancel",
      state: "canceled",
      paidAt: null,
    }));
    expect(result).toMatchObject({ checkoutStatus: "canceled", paymentId: null, creditId: null });
    const credits = await database.pool.query("SELECT count(*)::int AS count FROM engine_credits");
    expect(credits.rows[0]).toEqual({ count: 0 });
  });

  it("claims each credit once and records the engine balance", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const [first, second] = await Promise.all([
      claimNextCredit(database, "worker-a"),
      claimNextCredit(database, "worker-b"),
    ]);
    const claimed = first ?? second;
    expect(claimed).not.toBeNull();
    expect([first, second].filter(Boolean)).toHaveLength(1);

    await confirmCredit(database, claimed!.id, claimed!.leaseToken, 123_456_789n);
    const row = await database.pool.query("SELECT status, engine_balance_after_nano FROM engine_credits");
    expect(row.rows).toEqual([{ status: "confirmed", engine_balance_after_nano: "123456789" }]);
  });
});
