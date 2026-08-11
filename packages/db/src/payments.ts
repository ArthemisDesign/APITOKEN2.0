import { randomUUID } from "node:crypto";
import type { Database } from "./client.js";
import type { CheckoutState } from "./checkouts.js";

export type VerifiedPaymentState = "pending" | "paid" | "canceled" | "refunded";

export interface VerifiedCheckoutPaymentEvent {
  provider: string;
  providerEventId: string;
  providerPaymentId: string;
  checkoutId: string;
  state: VerifiedPaymentState;
  amountUsd: bigint;
  currency: "USD";
  paidAt: Date | null;
  payload: unknown;
}

export interface AppliedPaymentEvent {
  duplicateEvent: boolean;
  paymentId: string | null;
  creditId: string | null;
  checkoutStatus: CheckoutState | null;
}

/** Пополнение возвращаемо только в течение стольких дней с момента оплаты. */
export const REFUND_WINDOW_DAYS = 5;

export type RefundIneligibleReason =
  | "ok"
  | "not_found"
  | "not_paid"
  | "already_refunded"
  | "window_expired"
  | "balance_spent";

export interface RefundEligibility {
  eligible: boolean;
  reason: RefundIneligibleReason;
  paymentId: string | null;
  paidAt: string | null; // ISO
  windowDays: number;
  ageDays: number | null;
  realSpentSinceNano: string; // реальные деньги, потраченные ПОСЛЕ этого пополнения (nano); "0" = ничего
}

/**
 * Авторитетная проверка права на возврат по правилу пользовательского соглашения: пополнение
 * возвращаемо ТОЛЬКО если (а) с момента оплаты прошло ≤ REFUND_WINDOW_DAYS дней и (б) с этого
 * пополнения не потрачено НИ рубля реальных денег. «Потрачено реально» = сумма real_funded_nano по
 * списаниям после оплаты (бесплатное/промо в real_funded не входит — «бесплатное тратится первым»),
 * поэтому мелкие траты, покрытые бонусом, право на возврат не убивают, а любой реальный расход —
 * убивает. Только читает БД; ничего не проводит и не дебетует движок (исполнение возврата — отдельно).
 */
export async function evaluateRefundEligibility(
  database: Database,
  checkoutId: string,
  now: Date = new Date(),
  windowDays: number = REFUND_WINDOW_DAYS,
): Promise<RefundEligibility> {
  const base: RefundEligibility = {
    eligible: false, reason: "not_found", paymentId: null, paidAt: null,
    windowDays, ageDays: null, realSpentSinceNano: "0",
  };

  const paymentResult = await database.pool.query<{
    id: string; user_id: string; status: string; anchor: Date | null;
  }>(`
    SELECT id, user_id, status, COALESCE(paid_at, created_at) AS anchor
    FROM payments WHERE checkout_id = $1
  `, [checkoutId]);
  const payment = paymentResult.rows[0];
  if (!payment) return base;

  base.paymentId = payment.id;
  base.paidAt = payment.anchor ? payment.anchor.toISOString() : null;
  if (payment.status === "refunded" || payment.status === "disputed") {
    return { ...base, reason: "already_refunded" };
  }
  if (payment.status !== "paid" || !payment.anchor) {
    return { ...base, reason: "not_paid" };
  }

  const ageDays = (now.getTime() - payment.anchor.getTime()) / 86_400_000;
  base.ageDays = ageDays;
  if (ageDays > windowDays) return { ...base, reason: "window_expired" };

  const spentResult = await database.pool.query<{ real: string }>(`
    SELECT COALESCE(SUM(real_funded_nano), 0)::text AS real
    FROM pricing_usage_events
    WHERE user_id = $1 AND occurred_at >= $2
  `, [payment.user_id, payment.anchor]);
  const realSpent = BigInt(spentResult.rows[0]?.real ?? "0");
  base.realSpentSinceNano = realSpent.toString();
  if (realSpent > 0n) return { ...base, reason: "balance_spent" };

  return { ...base, eligible: true, reason: "ok" };
}

/** Applies an independently verified provider state and, if paid, enqueues one exact engine credit. */
export async function applyVerifiedCheckoutPaymentEvent(
  database: Database,
  input: VerifiedCheckoutPaymentEvent,
): Promise<AppliedPaymentEvent> {
  if (input.amountUsd <= 0n) throw new RangeError("payment amount must be positive whole USD");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const eventId = randomUUID();
    const event = await client.query<{ id: string }>(`
      INSERT INTO webhook_events (id, provider, provider_event_id, event_type, payload)
      VALUES ($1, $2, $3, $4, $5::jsonb)
      ON CONFLICT (provider, provider_event_id) DO NOTHING
      RETURNING id
    `, [
      eventId,
      input.provider,
      input.providerEventId,
      `payment.${input.state}`,
      JSON.stringify(input.payload),
    ]);
    if (!event.rows[0]) {
      await client.query("COMMIT");
      return { duplicateEvent: true, paymentId: null, creditId: null, checkoutStatus: null };
    }
    const webhookEventId = event.rows[0].id;

    const checkoutResult = await client.query<{
      user_id: string;
      engine_account_id: string;
      amount_usd: string;
      amount_nano: string;
      provider_payment_id: string | null;
      status: CheckoutState;
    }>(`
      SELECT user_id, engine_account_id, amount_usd, amount_nano, provider_payment_id, status
      FROM checkout_sessions
      WHERE id = $1 AND provider = $2
      FOR UPDATE
    `, [input.checkoutId, input.provider]);
    const checkout = checkoutResult.rows[0];
    if (!checkout) throw new Error("verified payment does not match a checkout session");
    if (checkout.provider_payment_id !== input.providerPaymentId) {
      throw new Error("verified payment ID does not match the checkout session");
    }
    if (BigInt(checkout.amount_usd) !== input.amountUsd) {
      throw new Error("verified payment amount does not match the checkout session");
    }

    let paymentId: string | null = null;
    let creditId: string | null = null;
    let checkoutStatus: CheckoutState = stateForCheckout(input.state);

    if (input.state === "paid" && checkout.status === "refunded") {
      // A delayed paid event must not recreate credit after an authoritative refund.
      checkoutStatus = "refunded";
    } else if (input.state === "paid") {
      const proposedPaymentId = randomUUID();
      const payment = await client.query<{
        id: string;
        checkout_id: string;
        user_id: string;
        amount_minor: string;
        amount_nano: string;
        currency: string;
      }>(`
        INSERT INTO payments (
          id, checkout_id, user_id, provider, provider_payment_id, amount_minor, currency,
          amount_nano, status, provider_state, paid_at
        ) VALUES ($1, $2, $3, $4, $5, $6, 'USD', $7, 'paid', $8::jsonb, COALESCE($9, now()))
        ON CONFLICT (provider, provider_payment_id) DO UPDATE
        SET updated_at = payments.updated_at
        RETURNING id, checkout_id, user_id, amount_minor, amount_nano, currency
      `, [
        proposedPaymentId,
        input.checkoutId,
        checkout.user_id,
        input.provider,
        input.providerPaymentId,
        (input.amountUsd * 100n).toString(),
        checkout.amount_nano,
        JSON.stringify({ last_event_id: input.providerEventId, raw: input.payload }),
        input.paidAt,
      ]);
      const stored = payment.rows[0];
      if (!stored) throw new Error("payment upsert returned no row");
      if (
        stored.checkout_id !== input.checkoutId ||
        stored.user_id !== checkout.user_id ||
        BigInt(stored.amount_minor) !== input.amountUsd * 100n ||
        BigInt(stored.amount_nano) !== BigInt(checkout.amount_nano) ||
        stored.currency !== "USD"
      ) {
        throw new Error("provider payment ID was reused with different payment data");
      }
      paymentId = stored.id;

      const proposedCreditId = randomUUID();
      const creditRef = `${input.provider}:${input.providerPaymentId}`;
      const credit = await client.query<{
        id: string;
        engine_account_id: string;
        amount_nano: string;
        idempotency_ref: string;
      }>(`
        INSERT INTO engine_credits (id, payment_id, engine_account_id, amount_nano, idempotency_ref)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (payment_id) DO UPDATE SET updated_at = engine_credits.updated_at
        RETURNING id, engine_account_id, amount_nano, idempotency_ref
      `, [proposedCreditId, stored.id, checkout.engine_account_id, checkout.amount_nano, creditRef]);
      const storedCredit = credit.rows[0];
      if (!storedCredit) throw new Error("engine credit upsert returned no row");
      if (
        storedCredit.engine_account_id !== checkout.engine_account_id ||
        BigInt(storedCredit.amount_nano) !== BigInt(checkout.amount_nano) ||
        storedCredit.idempotency_ref !== creditRef
      ) {
        throw new Error("payment already has a different engine credit");
      }
      creditId = storedCredit.id;
    } else if (input.state === "refunded") {
      const creditResult = await client.query<{
        payment_id: string;
        credit_id: string;
        engine_account_id: string;
        amount_nano: string;
        credit_status: "pending" | "processing" | "retry" | "confirmed" | "dead";
        credit_attempts: number;
      }>(`
        SELECT p.id AS payment_id, ec.id AS credit_id, ec.engine_account_id, ec.amount_nano,
               ec.status AS credit_status, ec.attempts AS credit_attempts
        FROM payments p
        JOIN engine_credits ec ON ec.payment_id = p.id
        WHERE p.checkout_id = $1
        FOR UPDATE OF p, ec
      `, [input.checkoutId]);
      const existingCredit = creditResult.rows[0];
      if (existingCredit) {
        const definitelyNeverDelivered = existingCredit.credit_attempts === 0 &&
          (existingCredit.credit_status === "pending" || existingCredit.credit_status === "dead");
        if (definitelyNeverDelivered) {
          await client.query(`
            UPDATE engine_credits
            SET status = 'dead', locked_at = NULL, locked_by = NULL,
                last_error = 'canceled because the provider payment was refunded', updated_at = now()
            WHERE id = $1
          `, [existingCredit.credit_id]);
        } else {
          // A claimed credit is ambiguous until it confirms: the engine may have applied the
          // idempotent top-up even if commerce saw a timeout. Keep/revive that credit so it first
          // reaches `confirmed`; the adjustment worker joins on that state before debiting. This
          // ordering guarantees exactly one top-up followed by exactly one compensation instead
          // of guessing whether a retry/processing attempt reached the engine.
          if (existingCredit.credit_status === "dead") {
            await client.query(`
              UPDATE engine_credits
              SET status = 'retry', next_attempt_at = now(), locked_at = NULL, locked_by = NULL,
                  last_error = 'revived to establish refund compensation ordering', updated_at = now()
              WHERE id = $1
            `, [existingCredit.credit_id]);
          } else if (existingCredit.credit_status === "retry") {
            await client.query(`
              UPDATE engine_credits
              SET next_attempt_at = now(), updated_at = now()
              WHERE id = $1 AND status = 'retry'
            `, [existingCredit.credit_id]);
          }

          const adjustmentId = randomUUID();
          const adjustmentRef = `refund:${existingCredit.payment_id}`;
          const adjustment = await client.query<{
            payment_id: string;
            engine_account_id: string;
            amount_nano: string;
            kind: "refund" | "dispute";
            idempotency_ref: string;
          }>(`
            INSERT INTO engine_adjustments (
              id, payment_id, webhook_event_id, engine_account_id, kind,
              amount_nano, idempotency_ref
            ) VALUES ($1, $2, $3, $4, 'refund', $5, $6)
            ON CONFLICT (idempotency_ref) DO UPDATE
            SET updated_at = engine_adjustments.updated_at
            RETURNING payment_id, engine_account_id, amount_nano, kind, idempotency_ref
          `, [
            adjustmentId,
            existingCredit.payment_id,
            webhookEventId,
            existingCredit.engine_account_id,
            (-BigInt(existingCredit.amount_nano)).toString(),
            adjustmentRef,
          ]);
          const stored = adjustment.rows[0];
          if (
            !stored ||
            stored.payment_id !== existingCredit.payment_id ||
            stored.engine_account_id !== existingCredit.engine_account_id ||
            BigInt(stored.amount_nano) !== -BigInt(existingCredit.amount_nano) ||
            stored.kind !== "refund" ||
            stored.idempotency_ref !== adjustmentRef
          ) {
            throw new Error("payment already has a different engine refund adjustment");
          }
        }
      }

      await client.query(`
        UPDATE payments SET status = 'refunded', provider_state = $2::jsonb, updated_at = now()
        WHERE checkout_id = $1
      `, [input.checkoutId, JSON.stringify({ last_event_id: input.providerEventId, raw: input.payload })]);
    }

    const checkoutUpdate = await client.query<{ status: CheckoutState }>(`
      UPDATE checkout_sessions
      SET status = CASE
            WHEN status IN ('paid', 'refunded') AND $2 IN ('pending', 'canceled') THEN status
            ELSE $2::checkout_status
          END,
          provider_state = $3::jsonb,
          completed_at = CASE WHEN $2 IN ('paid', 'canceled', 'refunded') THEN COALESCE(completed_at, now()) ELSE completed_at END,
          updated_at = now()
      WHERE id = $1
      RETURNING status
    `, [input.checkoutId, checkoutStatus, JSON.stringify({ last_event_id: input.providerEventId, raw: input.payload })]);
    checkoutStatus = checkoutUpdate.rows[0]?.status ?? checkoutStatus;

    await client.query(`
      UPDATE webhook_events SET status = 'processed', processed_at = now(), attempts = 1
      WHERE id = $1
    `, [eventId]);
    await client.query("COMMIT");
    return { duplicateEvent: false, paymentId, creditId, checkoutStatus };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

function stateForCheckout(state: VerifiedPaymentState): CheckoutState {
  return state === "canceled" ? "canceled" : state;
}
