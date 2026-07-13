import { randomUUID } from "node:crypto";
import type { Database } from "./client.js";

export interface VerifiedPaidWebhook {
  provider: string;
  providerEventId: string;
  eventType: string;
  providerPaymentId: string;
  userId: string;
  engineAccountId: string;
  amountMinor: bigint;
  amountNano: bigint;
  currency: string;
  payload: unknown;
}

export interface AcceptedPayment {
  duplicateEvent: boolean;
  paymentId: string | null;
  creditId: string | null;
}

/**
 * Persists an already signature-verified paid event and its engine credit in one transaction.
 * Provider adapters must never call this before verifying the raw webhook body.
 */
export async function acceptVerifiedPaidWebhook(
  database: Database,
  input: VerifiedPaidWebhook,
): Promise<AcceptedPayment> {
  if (input.amountMinor <= 0n || input.amountNano <= 0n) throw new RangeError("payment amounts must be positive");
  const currency = input.currency.toUpperCase();
  if (!/^[A-Z]{3}$/.test(currency)) throw new RangeError("currency must be a three-letter ISO code");

  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const eventId = randomUUID();
    const event = await client.query<{ id: string }>(`
      INSERT INTO webhook_events (id, provider, provider_event_id, event_type, payload)
      VALUES ($1, $2, $3, $4, $5::jsonb)
      ON CONFLICT (provider, provider_event_id) DO NOTHING
      RETURNING id
    `, [eventId, input.provider, input.providerEventId, input.eventType, JSON.stringify(input.payload)]);

    if (!event.rows[0]) {
      await client.query("COMMIT");
      return { duplicateEvent: true, paymentId: null, creditId: null };
    }

    const paymentId = randomUUID();
    const payment = await client.query<{
      id: string;
      user_id: string;
      amount_minor: string;
      amount_nano: string;
      currency: string;
    }>(`
      INSERT INTO payments (
        id, user_id, provider, provider_payment_id, amount_minor, currency,
        amount_nano, status, provider_state, paid_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, 'paid', $8::jsonb, now())
      ON CONFLICT (provider, provider_payment_id) DO UPDATE
      SET updated_at = payments.updated_at
      RETURNING id, user_id, amount_minor, amount_nano, currency
    `, [
      paymentId,
      input.userId,
      input.provider,
      input.providerPaymentId,
      input.amountMinor.toString(),
      currency,
      input.amountNano.toString(),
      JSON.stringify({ last_event_id: input.providerEventId }),
    ]);
    const stored = payment.rows[0];
    if (!stored) throw new Error("payment upsert returned no row");
    if (
      stored.user_id !== input.userId ||
      BigInt(stored.amount_minor) !== input.amountMinor ||
      BigInt(stored.amount_nano) !== input.amountNano ||
      stored.currency !== currency
    ) {
      throw new Error("provider payment ID was reused with different payment data");
    }

    const creditId = randomUUID();
    const creditRef = `${input.provider}:${input.providerPaymentId}`;
    const credit = await client.query<{
      id: string;
      engine_account_id: string;
      amount_nano: string;
      idempotency_ref: string;
    }>(`
      INSERT INTO engine_credits (
        id, payment_id, engine_account_id, amount_nano, idempotency_ref
      ) VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (payment_id) DO UPDATE
      SET updated_at = engine_credits.updated_at
      RETURNING id, engine_account_id, amount_nano, idempotency_ref
    `, [creditId, stored.id, input.engineAccountId, input.amountNano.toString(), creditRef]);
    const storedCredit = credit.rows[0];
    if (!storedCredit) throw new Error("engine credit upsert returned no row");
    if (
      storedCredit.engine_account_id !== input.engineAccountId ||
      BigInt(storedCredit.amount_nano) !== input.amountNano ||
      storedCredit.idempotency_ref !== creditRef
    ) {
      throw new Error("payment already has a different engine credit");
    }

    await client.query(`
      UPDATE webhook_events SET status = 'processed', processed_at = now(), attempts = 1
      WHERE id = $1
    `, [eventId]);
    await client.query("COMMIT");
    return { duplicateEvent: false, paymentId: stored.id, creditId: storedCredit.id };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
