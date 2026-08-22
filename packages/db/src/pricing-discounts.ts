import { randomUUID } from "node:crypto";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

/**
 * The engine provider ids a discount may target. Closed on purpose: a typo would be accepted,
 * stored and delivered, and would then silently never match a request.
 */
export const DISCOUNT_PROVIDER_IDS = ["anthropic", "openai", "google", "kimi", "glm"] as const;
export type DiscountProviderId = (typeof DISCOUNT_PROVIDER_IDS)[number];

export function isDiscountProviderId(value: string): value is DiscountProviderId {
  return (DISCOUNT_PROVIDER_IDS as readonly string[]).includes(value);
}

export class CustomerDiscountError extends Error {
  constructor(readonly code: "unknown_customer" | "invalid_provider" | "invalid_multiplier", message: string) {
    super(message);
    this.name = "CustomerDiscountError";
  }
}

/**
 * Queue one durable pricing delivery. `providerId: null` targets the account default multiplier;
 * a provider id targets that provider's override, and a null multiplier there removes it.
 *
 * One row per (user, target): a default change and a provider change are independent deliveries
 * and must not evict one another. An in-flight lease is never revoked — the desired value is
 * already durable in commerce, and `confirmPricingJob` requeues the row when it differs.
 */
export async function enqueuePricingJob(client: PoolClient, input: {
  userId: string;
  engineAccountId: string;
  providerId?: DiscountProviderId | null;
  multiplierBp: number | null;
  reason: string;
}): Promise<string> {
  const providerId = input.providerId ?? null;
  const existing = await client.query<{ id: string; status: "pending" | "processing" | "retry" | "confirmed" }>(`
    SELECT id, status FROM engine_pricing_jobs
    WHERE user_id = $1 AND provider_id IS NOT DISTINCT FROM $2
    FOR UPDATE
  `, [input.userId, providerId]);
  const row = existing.rows[0];
  if (row?.status === "processing") return row.id;
  if (row) {
    const updated = await client.query<{ id: string }>(`
      UPDATE engine_pricing_jobs SET
        engine_account_id = $2, multiplier_bp = $3, reason = $4,
        status = 'pending', attempts = 0, next_attempt_at = now(),
        locked_at = NULL, locked_by = NULL, last_error = NULL,
        confirmed_at = NULL, updated_at = now()
      WHERE id = $1
      RETURNING id
    `, [row.id, input.engineAccountId, input.multiplierBp, input.reason]);
    return updated.rows[0]!.id;
  }
  const id = randomUUID();
  await client.query(`
    INSERT INTO engine_pricing_jobs (
      id, user_id, engine_account_id, provider_id, multiplier_bp, reason
    ) VALUES ($1, $2, $3, $4, $5, $6)
  `, [id, input.userId, input.engineAccountId, providerId, input.multiplierBp, input.reason]);
  return id;
}

export interface CustomerProviderDiscount {
  providerId: DiscountProviderId;
  multiplierBp: number;
}

/** Every per-provider override of one customer, ordered for a stable admin view. */
export async function listCustomerProviderDiscounts(
  database: Database,
  userId: string,
): Promise<CustomerProviderDiscount[]> {
  const result = await database.pool.query<{ provider_id: string; multiplier_bp: number }>(`
    SELECT provider_id, multiplier_bp FROM customer_provider_discounts
    WHERE user_id = $1 ORDER BY provider_id
  `, [userId]);
  return result.rows
    .filter((row) => isDiscountProviderId(row.provider_id))
    .map((row) => ({
      providerId: row.provider_id as DiscountProviderId,
      multiplierBp: row.multiplier_bp,
    }));
}

/**
 * Set (`multiplierBp` a number) or clear (`null`) one provider override for a customer, and queue
 * its delivery in the same transaction. Commerce records what was asked for; the engine is the
 * authority that prices requests, and the job queue is what makes the two converge after an
 * engine outage. There is no version, no activation and nothing to keep in step.
 */
/**
 * One provider override applied inside a caller's transaction. Split out of
 * `setCustomerProviderDiscount` so an admin edit that changes the account default and several
 * providers at once commits as one fact: before this existed each leg opened its own transaction,
 * and a failure partway left the customer priced by a mixture of the old and new terms.
 */
export async function applyProviderDiscountTx(client: PoolClient, input: {
  userId: string;
  engineAccountId: string;
  providerId: string;
  multiplierBp: number | null;
  actorType?: "admin" | "sales";
  actorId: string;
  reason: string;
}): Promise<string> {
  if (!isDiscountProviderId(input.providerId)) {
    throw new CustomerDiscountError("invalid_provider", `unknown provider id: ${input.providerId}`);
  }
  if (input.multiplierBp !== null
    && (!Number.isInteger(input.multiplierBp) || input.multiplierBp < 0 || input.multiplierBp > 10_000)) {
    throw new CustomerDiscountError("invalid_multiplier", "multiplier_bp must be an integer between 0 and 10000");
  }
  if (input.multiplierBp === null) {
    await client.query(
      `DELETE FROM customer_provider_discounts WHERE user_id = $1 AND provider_id = $2`,
      [input.userId, input.providerId],
    );
  } else {
    await client.query(`
      INSERT INTO customer_provider_discounts (user_id, provider_id, multiplier_bp)
      VALUES ($1, $2, $3)
      ON CONFLICT (user_id, provider_id)
      DO UPDATE SET multiplier_bp = EXCLUDED.multiplier_bp, updated_at = now()
    `, [input.userId, input.providerId, input.multiplierBp]);
  }
  const jobId = await enqueuePricingJob(client, {
    userId: input.userId,
    engineAccountId: input.engineAccountId,
    providerId: input.providerId,
    multiplierBp: input.multiplierBp,
    reason: "provider_discount",
  });
  await client.query(`
    INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ($1, $2, 'pricing.provider_discount_changed', 'user', $3, $4::jsonb)
  `, [input.actorType ?? "admin", input.actorId, input.userId, JSON.stringify({
    providerId: input.providerId,
    multiplierBp: input.multiplierBp,
    reason: input.reason,
    jobId,
  })]);
  return jobId;
}

export async function setCustomerProviderDiscount(database: Database, input: {
  userId: string;
  providerId: string;
  multiplierBp: number | null;
  actorId: string;
  reason: string;
}): Promise<{ engineAccountId: string; jobId: string }> {
  if (!isDiscountProviderId(input.providerId)) {
    throw new CustomerDiscountError("invalid_provider", `unknown provider id: ${input.providerId}`);
  }
  if (input.multiplierBp !== null
    && (!Number.isInteger(input.multiplierBp) || input.multiplierBp < 0 || input.multiplierBp > 10_000)) {
    throw new CustomerDiscountError("invalid_multiplier", "multiplier_bp must be an integer between 0 and 10000");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const account = await client.query<{ engine_account_id: string }>(`
      SELECT ea.engine_account_id FROM engine_accounts ea
      WHERE ea.user_id = $1 AND ea.engine_account_id IS NOT NULL
      FOR UPDATE
    `, [input.userId]);
    const engineAccountId = account.rows[0]?.engine_account_id;
    if (!engineAccountId) {
      throw new CustomerDiscountError("unknown_customer", "customer has no engine account");
    }
    const jobId = await applyProviderDiscountTx(client, { ...input, engineAccountId });
    await client.query("COMMIT");
    return { engineAccountId, jobId };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
