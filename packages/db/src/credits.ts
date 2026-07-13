import { randomUUID } from "node:crypto";
import { sql } from "drizzle-orm";
import type { Database } from "./client.js";

export interface ClaimedCredit {
  id: string;
  paymentId: string;
  engineAccountId: string;
  amountNano: bigint;
  idempotencyRef: string;
  attempts: number;
}

export async function claimNextCredit(database: Database, workerId: string): Promise<ClaimedCredit | null> {
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
      SELECT id, payment_id, engine_account_id, amount_nano, idempotency_ref, attempts
      FROM engine_credits
      WHERE status IN ('pending', 'retry')
        AND next_attempt_at <= now()
      ORDER BY next_attempt_at, created_at
      FOR UPDATE SKIP LOCKED
      LIMIT 1
    `);
    const row = rows.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }

    await client.query(`
      UPDATE engine_credits
      SET status = 'processing', locked_at = now(), locked_by = $1,
          attempts = attempts + 1, updated_at = now()
      WHERE id = $2
    `, [workerId, row.id]);
    await client.query("COMMIT");
    return {
      id: row.id,
      paymentId: row.payment_id,
      engineAccountId: row.engine_account_id,
      amountNano: BigInt(row.amount_nano),
      idempotencyRef: row.idempotency_ref,
      attempts: row.attempts + 1,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function confirmCredit(database: Database, id: string, balanceNano: bigint): Promise<void> {
  await database.db.execute(sql`
    UPDATE engine_credits
    SET status = 'confirmed', engine_balance_after_nano = ${balanceNano.toString()},
        confirmed_at = now(), locked_at = NULL, locked_by = NULL,
        last_error = NULL, updated_at = now()
    WHERE id = ${id} AND status = 'processing'
  `);
}

export async function retryCredit(database: Database, id: string, error: string, attempts: number): Promise<void> {
  const cappedError = error.slice(0, 2000);
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(attempts, 10)));
  await database.db.execute(sql`
    UPDATE engine_credits
    SET status = 'retry', next_attempt_at = now() + (${delaySeconds} * interval '1 second'),
        locked_at = NULL, locked_by = NULL, last_error = ${cappedError}, updated_at = now()
    WHERE id = ${id} AND status = 'processing'
  `);
}

export async function recoverStaleCredits(database: Database): Promise<number> {
  const result = await database.db.execute(sql`
    UPDATE engine_credits
    SET status = 'retry', locked_at = NULL, locked_by = NULL,
        next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}

export function newCreditId(): string {
  return randomUUID();
}
