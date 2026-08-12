import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  activateCheckoutSession,
  applyVerifiedCheckoutPaymentEvent,
  claimNextAdjustment,
  claimNextCredit,
  confirmAdjustment,
  confirmCredit,
  createCheckoutSession,
  createDatabase,
  getCheckoutSession,
  recoverStaleAdjustments,
  retryAdjustment,
  retryCredit,
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
      TRUNCATE audit_log, api_keys, engine_adjustments, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
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
      TRUNCATE audit_log, api_keys, engine_adjustments, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  async function checkout(
    amountUsd = 25n,
    providerPaymentId = "26109ba0-b05b-4ee0-93d1-fd62c822ce95",
  ): Promise<CheckoutSession> {
    const created = await createCheckoutSession(database, { userId, provider: "cryptomus", amountUsd });
    return activateCheckoutSession(database, {
      id: created.id,
      providerPaymentId,
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

  function refundEvent(
    session: CheckoutSession,
    providerEventId = "payment-1:refunded",
  ): VerifiedCheckoutPaymentEvent {
    return event(session, {
      providerEventId,
      state: "refunded",
      paidAt: null,
      payload: { safe: "refund-fixture" },
    });
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

  it("serializes the paid feed sequence against an in-flight legacy insert", async () => {
    const legacy = await checkout(25n, "10000000-0000-4000-8000-000000000001");
    const current = await checkout(25n, "10000000-0000-4000-8000-000000000002");
    const blocker = await database.pool.connect();
    let recording: Promise<unknown> | undefined;
    try {
      await blocker.query("BEGIN");
      await blocker.query(`
        INSERT INTO payments (
          id, checkout_id, user_id, provider, provider_payment_id, amount_minor, currency,
          amount_nano, status, provider_state, paid_at
        ) VALUES ($1, $2, $3, 'cryptomus', $4, 2500, 'USD', 25000000000, 'paid', '{}', now())
      `, [randomUUID(), legacy.id, userId, legacy.providerPaymentId]);

      recording = applyVerifiedCheckoutPaymentEvent(database, event(current, {
        providerEventId: "payment-2:paid",
      }));
      let observedWait = false;
      for (let attempt = 0; attempt < 50 && !observedWait; attempt += 1) {
        const locks = await database.pool.query<{ waiting: boolean }>(`
          SELECT EXISTS (
            SELECT 1 FROM pg_locks
            WHERE relation = 'payments'::regclass
              AND mode = 'ShareRowExclusiveLock' AND NOT granted
          ) AS waiting
        `);
        observedWait = locks.rows[0]?.waiting ?? false;
        if (!observedWait) await new Promise((resolve) => setTimeout(resolve, 20));
      }
      if (!observedWait) {
        await blocker.query("ROLLBACK");
        await recording;
      }
      expect(observedWait).toBe(true);

      await blocker.query("COMMIT");
      await recording;
      const rows = await database.pool.query<{ provider_payment_id: string; feed_seq: string }>(`
        SELECT provider_payment_id, feed_seq::text FROM payments ORDER BY feed_seq
      `);
      expect(rows.rows).toEqual([
        { provider_payment_id: legacy.providerPaymentId, feed_seq: "1" },
        { provider_payment_id: current.providerPaymentId, feed_seq: "2" },
      ]);
    } finally {
      await blocker.query("ROLLBACK").catch(() => undefined);
      blocker.release();
      await recording?.catch(() => undefined);
    }
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

  it("refunds without compensation when the positive credit was never claimed", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));

    await expect(
      applyVerifiedCheckoutPaymentEvent(database, refundEvent(session)),
    ).resolves.toMatchObject({ checkoutStatus: "refunded", paymentId: null, creditId: null });

    const state = await database.pool.query(`
      SELECT
        (SELECT status FROM payments WHERE checkout_id = $1) AS payment_status,
        (SELECT status FROM checkout_sessions WHERE id = $1) AS checkout_status,
        (SELECT ec.status FROM engine_credits ec
           JOIN payments p ON p.id = ec.payment_id
          WHERE p.checkout_id = $1) AS credit_status,
        (SELECT last_error FROM engine_credits ec
           JOIN payments p ON p.id = ec.payment_id
          WHERE p.checkout_id = $1) AS last_error,
        (SELECT count(*)::int FROM engine_adjustments) AS adjustments
    `, [session.id]);
    expect(state.rows[0]).toEqual({
      payment_status: "refunded",
      checkout_status: "refunded",
      credit_status: "dead",
      last_error: "canceled because the provider payment was refunded",
      adjustments: 0,
    });
  });

  it("records compensation for an ambiguous retry and waits for its positive credit", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const firstAttempt = await claimNextCredit(database, "worker-retry");
    expect(firstAttempt).not.toBeNull();
    await expect(
      retryCredit(
        database,
        firstAttempt!.id,
        firstAttempt!.leaseToken,
        "response lost after possible engine commit",
        firstAttempt!.attempts,
      ),
    ).resolves.toBe(true);

    await expect(
      applyVerifiedCheckoutPaymentEvent(database, refundEvent(session)),
    ).resolves.toMatchObject({ checkoutStatus: "refunded" });
    await expect(claimNextAdjustment(database, "adjustment-too-early")).resolves.toBeNull();

    const state = await database.pool.query(`
      SELECT credit.status AS credit_status, adjustment.status AS adjustment_status,
             adjustment.amount_nano, adjustment.idempotency_ref
      FROM payments payment
      JOIN engine_credits credit ON credit.payment_id = payment.id
      JOIN engine_adjustments adjustment ON adjustment.payment_id = payment.id
      WHERE payment.checkout_id = $1
    `, [session.id]);
    expect(state.rows[0]).toEqual({
      credit_status: "retry",
      adjustment_status: "pending",
      amount_nano: "-25000000000",
      idempotency_ref: `refund:${firstAttempt!.paymentId}`,
    });

    const replay = await claimNextCredit(database, "worker-replay");
    expect(replay).not.toBeNull();
    await expect(confirmCredit(database, replay!.id, replay!.leaseToken, 25_000_000_000n))
      .resolves.toBe(true);
    const adjustment = await claimNextAdjustment(database, "adjustment-after-credit");
    expect(adjustment).toMatchObject({
      paymentId: firstAttempt!.paymentId,
      amountNano: 25_000_000_000n,
      idempotencyRef: `refund:${firstAttempt!.paymentId}`,
    });
  });

  it("finalizes a refund and durably queues compensation for a confirmed engine credit", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const claimed = await claimNextCredit(database, "worker-confirm");
    expect(claimed).not.toBeNull();
    await expect(
      confirmCredit(database, claimed!.id, claimed!.leaseToken, 25_000_000_000n),
    ).resolves.toBe(true);

    await expect(
      applyVerifiedCheckoutPaymentEvent(database, refundEvent(session)),
    ).resolves.toMatchObject({ checkoutStatus: "refunded", paymentId: null, creditId: null });

    const state = await database.pool.query(`
      SELECT
        (SELECT status FROM payments WHERE checkout_id = $1) AS payment_status,
        (SELECT status FROM checkout_sessions WHERE id = $1) AS checkout_status,
        (SELECT ec.status FROM engine_credits ec
           JOIN payments p ON p.id = ec.payment_id
          WHERE p.checkout_id = $1) AS credit_status,
        (SELECT status FROM engine_adjustments) AS adjustment_status,
        (SELECT amount_nano FROM engine_adjustments) AS adjustment_amount,
        (SELECT count(*)::int FROM audit_log
          WHERE action = 'payment.reversed') AS reversal_events,
        (SELECT metadata FROM audit_log
          WHERE action = 'payment.reversed') AS reversal_metadata,
        (SELECT count(*)::int FROM webhook_events
          WHERE provider_event_id = 'payment-1:refunded') AS refund_events
    `, [session.id]);
    expect(state.rows[0]).toEqual({
      payment_status: "refunded",
      checkout_status: "refunded",
      credit_status: "confirmed",
      adjustment_status: "pending",
      adjustment_amount: "-25000000000",
      reversal_events: 1,
      reversal_metadata: {
        kind: "refund",
        amountNano: "25000000000",
        providerEventId: "payment-1:refunded",
      },
      refund_events: 1,
    });

    const adjustment = await claimNextAdjustment(database, "adjustment-confirm");
    expect(adjustment).not.toBeNull();
    await expect(
      confirmAdjustment(database, adjustment!.id, adjustment!.leaseToken, -123n),
    ).resolves.toBe(true);
    const confirmed = await database.pool.query(
      "SELECT status, engine_balance_after_nano FROM engine_adjustments",
    );
    expect(confirmed.rows).toEqual([{ status: "confirmed", engine_balance_after_nano: "-123" }]);
  });

  it("orders reversal evidence after an in-flight legacy audit insert", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const blocker = await database.pool.connect();
    let refunding: Promise<unknown> | undefined;
    try {
      await blocker.query("BEGIN");
      const legacy = await blocker.query<{ id: string }>(`
        INSERT INTO audit_log (actor_type, action, target_type, target_id)
        VALUES ('system', 'legacy.concurrent', 'test', 'legacy-before-reversal')
        RETURNING id::text
      `);

      refunding = applyVerifiedCheckoutPaymentEvent(database, refundEvent(session));
      let observedWait = false;
      for (let attempt = 0; attempt < 50 && !observedWait; attempt += 1) {
        const locks = await database.pool.query<{ waiting: boolean }>(`
          SELECT EXISTS (
            SELECT 1 FROM pg_locks
            WHERE relation = 'audit_log'::regclass
              AND mode = 'ShareRowExclusiveLock' AND NOT granted
          ) AS waiting
        `);
        observedWait = locks.rows[0]?.waiting ?? false;
        if (!observedWait) await new Promise((resolve) => setTimeout(resolve, 20));
      }
      if (!observedWait) {
        await blocker.query("ROLLBACK");
        await refunding;
      }
      expect(observedWait).toBe(true);

      await blocker.query("COMMIT");
      await refunding;
      const rows = await database.pool.query<{ id: string; action: string }>(`
        SELECT id::text, action
        FROM audit_log
        WHERE action IN ('legacy.concurrent', 'payment.reversed')
        ORDER BY id
      `);
      expect(rows.rows).toEqual([
        { id: legacy.rows[0]!.id, action: "legacy.concurrent" },
        { id: (BigInt(legacy.rows[0]!.id) + 1n).toString(), action: "payment.reversed" },
      ]);
    } finally {
      await blocker.query("ROLLBACK").catch(() => undefined);
      blocker.release();
      await refunding?.catch(() => undefined);
    }
  });

  it("deduplicates distinct refund events onto one payment compensation", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const credit = await claimNextCredit(database, "worker-confirm");
    await confirmCredit(database, credit!.id, credit!.leaseToken, 25_000_000_000n);

    await applyVerifiedCheckoutPaymentEvent(database, refundEvent(session, "refund-event-one"));
    const adjustment = await claimNextAdjustment(database, "adjustment-dedupe");
    await confirmAdjustment(database, adjustment!.id, adjustment!.leaseToken, 0n);
    await applyVerifiedCheckoutPaymentEvent(database, refundEvent(session, "refund-event-two"));

    const counts = await database.pool.query(`
      SELECT (SELECT count(*)::int FROM engine_adjustments) AS adjustments,
             (SELECT status FROM engine_adjustments) AS adjustment_status,
             (SELECT count(*)::int FROM audit_log
               WHERE action = 'payment.reversed') AS reversals,
             (SELECT count(*)::int FROM webhook_events WHERE event_type = 'payment.refunded') AS events
    `);
    expect(counts.rows[0]).toEqual({
      adjustments: 1,
      adjustment_status: "confirmed",
      reversals: 1,
      events: 2,
    });
  });

  it("fences a stale adjustment worker after lease recovery", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    const credit = await claimNextCredit(database, "worker-confirm");
    await confirmCredit(database, credit!.id, credit!.leaseToken, 25_000_000_000n);
    await applyVerifiedCheckoutPaymentEvent(database, refundEvent(session));

    const stale = await claimNextAdjustment(database, "adjustment-stale");
    expect(stale).not.toBeNull();
    await database.pool.query(
      "UPDATE engine_adjustments SET locked_at = now() - interval '10 minutes' WHERE id = $1",
      [stale!.id],
    );
    await expect(recoverStaleAdjustments(database)).resolves.toBe(1);
    const current = await claimNextAdjustment(database, "adjustment-current");
    expect(current).not.toBeNull();
    expect(current!.leaseToken).not.toBe(stale!.leaseToken);

    await expect(
      retryAdjustment(database, stale!.id, stale!.leaseToken, "late stale failure", stale!.attempts),
    ).resolves.toBe(false);
    await expect(confirmAdjustment(database, stale!.id, stale!.leaseToken, 1n)).resolves.toBe(false);
    await expect(confirmAdjustment(database, current!.id, current!.leaseToken, 2n)).resolves.toBe(true);
  });

  it("never recreates credit when a paid event arrives after a refund", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));
    await applyVerifiedCheckoutPaymentEvent(database, refundEvent(session));

    const delayedPaid = await applyVerifiedCheckoutPaymentEvent(database, event(session, {
      providerEventId: "payment-1:delayed-paid",
      payload: { safe: "delayed-paid-fixture" },
    }));
    expect(delayedPaid).toEqual({
      duplicateEvent: false,
      paymentId: null,
      creditId: null,
      checkoutStatus: "refunded",
    });

    const state = await database.pool.query(`
      SELECT
        (SELECT status FROM payments WHERE checkout_id = $1) AS payment_status,
        (SELECT status FROM checkout_sessions WHERE id = $1) AS checkout_status,
        (SELECT count(*)::int FROM engine_credits ec
           JOIN payments p ON p.id = ec.payment_id
          WHERE p.checkout_id = $1) AS credit_count,
        (SELECT ec.status FROM engine_credits ec
           JOIN payments p ON p.id = ec.payment_id
          WHERE p.checkout_id = $1) AS credit_status
    `, [session.id]);
    expect(state.rows[0]).toEqual({
      payment_status: "refunded",
      checkout_status: "refunded",
      credit_count: 1,
      credit_status: "dead",
    });
  });

  it("does not let a pending provider event revert a paid checkout", async () => {
    const session = await checkout();
    await applyVerifiedCheckoutPaymentEvent(database, event(session));

    const pending = await applyVerifiedCheckoutPaymentEvent(database, event(session, {
      providerEventId: "payment-1:pending-after-paid",
      state: "pending",
      paidAt: null,
    }));
    expect(pending).toMatchObject({ checkoutStatus: "paid", paymentId: null, creditId: null });
    await expect(
      getCheckoutSession(database, { id: session.id, userId }),
    ).resolves.toMatchObject({ status: "paid" });
  });

  it("serializes racing paid and refunded events without leaving creditable work", async () => {
    const session = await checkout();
    const [paid, refunded] = await Promise.all([
      applyVerifiedCheckoutPaymentEvent(database, event(session, {
        providerEventId: "payment-1:racing-paid",
      })),
      applyVerifiedCheckoutPaymentEvent(database, refundEvent(session, "payment-1:racing-refund")),
    ]);
    expect([paid.checkoutStatus, refunded.checkoutStatus]).toContain("refunded");

    const state = await database.pool.query(`
      SELECT
        (SELECT status FROM checkout_sessions WHERE id = $1) AS checkout_status,
        (SELECT count(*)::int
           FROM engine_credits ec
           JOIN payments p ON p.id = ec.payment_id
          WHERE p.checkout_id = $1
            AND ec.status IN ('pending', 'retry', 'processing', 'confirmed')) AS creditable_count
    `, [session.id]);
    expect(state.rows[0]).toEqual({ checkout_status: "refunded", creditable_count: 0 });
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
