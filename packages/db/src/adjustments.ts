import { randomUUID } from "node:crypto";
import { sql } from "drizzle-orm";
import type { Database } from "./client.js";

export interface ClaimedAdjustment {
  id: string;
  paymentId: string;
  engineAccountId: string;
  /** Positive magnitude passed to EngineClient.debitAccount; storage keeps the signed debit. */
  amountNano: bigint;
  idempotencyRef: string;
  attempts: number;
  leaseToken: string;
}

/**
 * Claims only compensations whose positive credit is durably confirmed. A refund can race an
 * in-flight/retrying credit, so the adjustment is recorded immediately but must never debit the
 * engine before the original idempotent top-up is known to have landed.
 */
export async function claimNextAdjustment(
  database: Database,
  workerId: string,
): Promise<ClaimedAdjustment | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const rows = await client.query<{
      id: string;
      payment_id: string;
      engine_account_id: string;
      amount_nano: string;
      idempotency_ref: string;
      attempts: number;
    }>(`
      SELECT adjustment.id, adjustment.payment_id, adjustment.engine_account_id,
             adjustment.amount_nano, adjustment.idempotency_ref, adjustment.attempts
      FROM engine_adjustments adjustment
      JOIN engine_credits credit ON credit.payment_id = adjustment.payment_id
      JOIN payments payment ON payment.id = adjustment.payment_id
      WHERE adjustment.status IN ('pending', 'retry')
        AND adjustment.next_attempt_at <= now()
        AND credit.status = 'confirmed'
        AND payment.status IN ('refunded', 'disputed')
      ORDER BY adjustment.next_attempt_at, adjustment.created_at
      FOR UPDATE OF adjustment SKIP LOCKED
      LIMIT 1
    `);
    const row = rows.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }

    const storedAmount = BigInt(row.amount_nano);
    if (storedAmount >= 0n) {
      throw new Error(`engine adjustment ${row.id} has a non-negative amount`);
    }
    const leaseToken = `${workerId}:${randomUUID()}`;
    const claimed = await client.query(`
      UPDATE engine_adjustments
      SET status = 'processing', locked_at = now(), locked_by = $1,
          attempts = attempts + 1, updated_at = now()
      WHERE id = $2 AND status IN ('pending', 'retry')
    `, [leaseToken, row.id]);
    if (claimed.rowCount !== 1) {
      throw new Error(`engine adjustment ${row.id} lost its row lock while being claimed`);
    }
    await client.query("COMMIT");
    return {
      id: row.id,
      paymentId: row.payment_id,
      engineAccountId: row.engine_account_id,
      amountNano: -storedAmount,
      idempotencyRef: row.idempotency_ref,
      attempts: row.attempts + 1,
      leaseToken,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function confirmAdjustment(
  database: Database,
  id: string,
  leaseToken: string,
  balanceNano: bigint,
): Promise<boolean> {
  const result = await database.db.execute(sql`
    UPDATE engine_adjustments
    SET status = 'confirmed', engine_balance_after_nano = ${balanceNano.toString()},
        confirmed_at = now(), locked_at = NULL, locked_by = NULL,
        last_error = NULL, updated_at = now()
    WHERE id = ${id} AND status = 'processing' AND locked_by = ${leaseToken}
  `);
  return result.rowCount === 1;
}

export async function retryAdjustment(
  database: Database,
  id: string,
  leaseToken: string,
  error: string,
  attempts: number,
): Promise<boolean> {
  const cappedError = error.slice(0, 2000);
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(attempts, 10)));
  const result = await database.db.execute(sql`
    UPDATE engine_adjustments
    SET status = 'retry', next_attempt_at = now() + (${delaySeconds} * interval '1 second'),
        locked_at = NULL, locked_by = NULL, last_error = ${cappedError}, updated_at = now()
    WHERE id = ${id} AND status = 'processing' AND locked_by = ${leaseToken}
  `);
  return result.rowCount === 1;
}

export async function recoverStaleAdjustments(database: Database): Promise<number> {
  const result = await database.db.execute(sql`
    UPDATE engine_adjustments
    SET status = 'retry', locked_at = NULL, locked_by = NULL,
        next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}
