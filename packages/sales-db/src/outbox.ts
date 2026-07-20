import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";
import type { PartnerAuthPurpose } from "./auth.js";

export interface ClaimedPartnerEmail {
  id: string;
  recipient: string;
  template: PartnerAuthPurpose;
  encryptedToken: string;
  attempts: number;
}

export async function insertPartnerEmail(
  client: PoolClient,
  input: {
    partnerId: string;
    recipient: string;
    purpose: PartnerAuthPurpose;
    tokenHash: string;
    encryptedToken: string;
    expiresAt: Date;
  },
): Promise<void> {
  await client.query(`
    INSERT INTO partner_auth_tokens (partner_id, purpose, token_hash, expires_at)
    VALUES ($1, $2, $3, $4)
  `, [input.partnerId, input.purpose, input.tokenHash, input.expiresAt]);
  await client.query(`
    INSERT INTO partner_email_outbox (partner_id, recipient, template, payload)
    VALUES ($1, $2, $3, $4::jsonb)
  `, [
    input.partnerId, input.recipient, input.purpose,
    JSON.stringify({ encryptedToken: input.encryptedToken }),
  ]);
}

export async function claimNextPartnerEmail(database: SalesDatabase, workerId: string): Promise<ClaimedPartnerEmail | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(`
      UPDATE partner_email_outbox SET status = 'failed', last_error = 'invalid encrypted token payload', updated_at = now()
      WHERE status = 'pending'
        AND (NOT (payload ? 'encryptedToken') OR jsonb_typeof(payload->'encryptedToken') <> 'string')
    `);
    const result = await client.query<{
      id: string; recipient: string; template: PartnerAuthPurpose; payload: unknown; attempts: number;
    }>(`
      SELECT id, recipient, template, payload, attempts FROM partner_email_outbox
      WHERE status = 'pending' AND next_attempt_at <= now()
      ORDER BY next_attempt_at, created_at FOR UPDATE SKIP LOCKED LIMIT 1
    `);
    const row = result.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }
    const encryptedToken = readEncryptedToken(row.payload);
    await client.query(`
      UPDATE partner_email_outbox SET status = 'sending', attempts = attempts + 1,
        locked_at = now(), locked_by = $2, updated_at = now() WHERE id = $1
    `, [row.id, workerId]);
    await client.query("COMMIT");
    return { id: row.id, recipient: row.recipient, template: row.template, encryptedToken, attempts: row.attempts + 1 };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function confirmPartnerEmail(database: SalesDatabase, id: string, lockedBy: string): Promise<void> {
  // Только держатель текущей аренды (locked_by) может закрыть строку — иначе после recover-а stale-аренды
  // и повторного захвата другим воркером старый воркер затирал бы чужой прогресс (дубли писем).
  await database.pool.query(`
    UPDATE partner_email_outbox SET status = 'sent', sent_at = now(),
      locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
    WHERE id = $1 AND status = 'sending' AND locked_by = $2
  `, [id, lockedBy]);
}

export async function retryPartnerEmail(database: SalesDatabase, job: ClaimedPartnerEmail, error: string, lockedBy: string): Promise<void> {
  const terminal = job.attempts >= 10;
  const delaySeconds = Math.min(3600, Math.max(10, 2 ** Math.min(job.attempts, 10)));
  await database.pool.query(`
    UPDATE partner_email_outbox SET status = $2, next_attempt_at = now() + ($3 * interval '1 second'),
      locked_at = NULL, locked_by = NULL, last_error = $4, updated_at = now()
    WHERE id = $1 AND status = 'sending' AND locked_by = $5
  `, [job.id, terminal ? "failed" : "pending", delaySeconds, error.slice(0, 2000), lockedBy]);
}

export async function recoverStalePartnerEmails(database: SalesDatabase): Promise<number> {
  const result = await database.pool.query(`
    UPDATE partner_email_outbox SET status = 'pending', locked_at = NULL, locked_by = NULL,
      next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'sending' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}

function readEncryptedToken(payload: unknown): string {
  if (typeof payload !== "object" || payload === null || !("encryptedToken" in payload) ||
      typeof (payload as { encryptedToken: unknown }).encryptedToken !== "string") {
    throw new Error("partner email outbox payload has no encrypted token");
  }
  return (payload as { encryptedToken: string }).encryptedToken;
}
