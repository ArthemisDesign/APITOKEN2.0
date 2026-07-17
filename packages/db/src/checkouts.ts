import { randomUUID } from "node:crypto";
import type { Database } from "./client.js";

const NANO_USD = 1_000_000_000n;

export type CheckoutState = "creating" | "pending" | "paid" | "canceled" | "refunded" | "failed";

export interface CheckoutSession {
  id: string;
  userId: string;
  customerEmail: string;
  engineAccountId: string;
  provider: string;
  amountUsd: bigint;
  amountNano: bigint;
  providerPaymentId: string | null;
  checkoutUrl: string | null;
  status: CheckoutState;
  expiresAt: Date | null;
}

export async function createCheckoutSession(
  database: Database,
  input: { userId: string; provider: string; amountUsd: bigint },
): Promise<CheckoutSession> {
  if (input.amountUsd <= 0n) throw new RangeError("checkout amount must be positive whole USD");
  const id = randomUUID();
  const result = await database.pool.query<CheckoutRow>(`
    INSERT INTO checkout_sessions (id, user_id, engine_account_id, provider, amount_usd, amount_nano)
    SELECT $1, u.id, ea.engine_account_id, $2, $3, $4
    FROM users u
    JOIN engine_accounts ea ON ea.user_id = u.id
    WHERE u.id = $5 AND u.status = 'active'
      AND ea.status = 'active' AND ea.engine_account_id IS NOT NULL
    RETURNING id, user_id, (SELECT email FROM users WHERE users.id = checkout_sessions.user_id) AS customer_email,
              engine_account_id, provider, amount_usd, amount_nano,
              provider_payment_id, checkout_url, status, expires_at
  `, [id, input.provider, input.amountUsd.toString(), (input.amountUsd * NANO_USD).toString(), input.userId]);
  const row = result.rows[0];
  if (!row) throw new Error("active user with an active engine account was not found");
  return mapCheckout(row);
}

export async function activateCheckoutSession(
  database: Database,
  input: { id: string; providerPaymentId: string; checkoutUrl: string; expiresAt?: Date | null; providerState: unknown },
): Promise<CheckoutSession> {
  const result = await database.pool.query<CheckoutRow>(`
    UPDATE checkout_sessions
    SET provider_payment_id = $2, checkout_url = $3, expires_at = $4,
        provider_state = $5::jsonb, status = 'pending', updated_at = now()
    WHERE id = $1 AND status = 'creating'
    RETURNING id, user_id, (SELECT email FROM users WHERE users.id = checkout_sessions.user_id) AS customer_email,
              engine_account_id, provider, amount_usd, amount_nano,
              provider_payment_id, checkout_url, status, expires_at
  `, [input.id, input.providerPaymentId, input.checkoutUrl, input.expiresAt ?? null, JSON.stringify(input.providerState)]);
  const row = result.rows[0];
  if (!row) throw new Error("checkout session is not in creating state");
  return mapCheckout(row);
}

export async function failCheckoutSession(database: Database, id: string, error: string): Promise<void> {
  await database.pool.query(`
    UPDATE checkout_sessions
    SET status = 'failed', provider_state = jsonb_build_object('error', $2::text), updated_at = now()
    WHERE id = $1 AND status = 'creating'
  `, [id, error.slice(0, 1000)]);
}

export async function getCheckoutSession(
  database: Database,
  input: { id: string; userId: string },
): Promise<CheckoutSession | null> {
  const result = await database.pool.query<CheckoutRow>(`
    SELECT cs.id, cs.user_id, u.email AS customer_email, cs.engine_account_id, cs.provider, cs.amount_usd, cs.amount_nano,
           cs.provider_payment_id, cs.checkout_url, cs.status, cs.expires_at
    FROM checkout_sessions cs JOIN users u ON u.id = cs.user_id
    WHERE cs.id = $1 AND cs.user_id = $2
  `, [input.id, input.userId]);
  return result.rows[0] ? mapCheckout(result.rows[0]) : null;
}

// Reconciliation source: still-pending checkouts that already have a provider payment id, so a
// poller can re-verify them against the provider and credit confirmed ones the webhook missed.
// minAgeSeconds gives the webhook a head start; maxAgeSeconds bounds re-querying dead checkouts.
export async function listPendingCheckoutsForReconcile(
  database: Database,
  input: { provider: string; minAgeSeconds: number; maxAgeSeconds: number; limit: number },
): Promise<Array<{ id: string; providerPaymentId: string; amountUsd: bigint }>> {
  const result = await database.pool.query<{ id: string; provider_payment_id: string; amount_usd: string }>(`
    SELECT id, provider_payment_id, amount_usd
    FROM checkout_sessions
    WHERE provider = $1 AND status = 'pending' AND provider_payment_id IS NOT NULL
      AND created_at <= now() - ($2 * interval '1 second')
      AND created_at >= now() - ($3 * interval '1 second')
    ORDER BY created_at ASC
    LIMIT $4
  `, [input.provider, input.minAgeSeconds, input.maxAgeSeconds, input.limit]);
  return result.rows.map((row) => ({
    id: row.id,
    providerPaymentId: row.provider_payment_id,
    amountUsd: BigInt(row.amount_usd),
  }));
}

// Webhook-facing lookup: no user session is available, so the (provider, provider_payment_id)
// unique pair identifies the checkout whose authoritative recorded USD must be credited.
export async function getCheckoutByProviderPayment(
  database: Database,
  input: { provider: string; providerPaymentId: string },
): Promise<CheckoutSession | null> {
  const result = await database.pool.query<CheckoutRow>(`
    SELECT cs.id, cs.user_id, u.email AS customer_email, cs.engine_account_id, cs.provider, cs.amount_usd, cs.amount_nano,
           cs.provider_payment_id, cs.checkout_url, cs.status, cs.expires_at
    FROM checkout_sessions cs JOIN users u ON u.id = cs.user_id
    WHERE cs.provider = $1 AND cs.provider_payment_id = $2
  `, [input.provider, input.providerPaymentId]);
  return result.rows[0] ? mapCheckout(result.rows[0]) : null;
}

interface CheckoutRow {
  id: string;
  user_id: string;
  customer_email: string;
  engine_account_id: string;
  provider: string;
  amount_usd: string;
  amount_nano: string;
  provider_payment_id: string | null;
  checkout_url: string | null;
  status: CheckoutState;
  expires_at: Date | null;
}

function mapCheckout(row: CheckoutRow): CheckoutSession {
  return {
    id: row.id,
    userId: row.user_id,
    customerEmail: row.customer_email,
    engineAccountId: row.engine_account_id,
    provider: row.provider,
    amountUsd: BigInt(row.amount_usd),
    amountNano: BigInt(row.amount_nano),
    providerPaymentId: row.provider_payment_id,
    checkoutUrl: row.checkout_url,
    status: row.status,
    expiresAt: row.expires_at,
  };
}
