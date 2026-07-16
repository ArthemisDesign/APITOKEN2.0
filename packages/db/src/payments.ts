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
        credit_id: string;
        credit_status: "pending" | "processing" | "retry" | "confirmed" | "dead";
      }>(`
        SELECT ec.id AS credit_id, ec.status AS credit_status
        FROM payments p
        JOIN engine_credits ec ON ec.payment_id = p.id
        WHERE p.checkout_id = $1
        FOR UPDATE OF ec
      `, [input.checkoutId]);
      const existingCredit = creditResult.rows[0];
      if (existingCredit) {
        if (existingCredit.credit_status === "pending" || existingCredit.credit_status === "retry") {
          await client.query(`
            UPDATE engine_credits
            SET status = 'dead', locked_at = NULL, locked_by = NULL,
                last_error = 'canceled because the provider payment was refunded', updated_at = now()
            WHERE id = $1
          `, [existingCredit.credit_id]);
        } else {
          // AUDIT-TODO(C14): add a durable idempotent engine debit/reversal and worker lease protocol.
          throw new Error("cannot finalize refund until engine credit compensation is durably recorded");
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
