import { randomUUID } from "node:crypto";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

export type AuthEmailPurpose = "verify_email" | "reset_password";

export interface ClaimedEmail {
  id: string;
  recipient: string;
  template: AuthEmailPurpose;
  encryptedToken: string;
  attempts: number;
}

export async function insertAuthEmail(
  client: PoolClient,
  input: {
    userId: string;
    recipient: string;
    purpose: AuthEmailPurpose;
    tokenHash: string;
    encryptedToken: string;
    expiresAt: Date;
  },
): Promise<void> {
  await client.query(`
    INSERT INTO auth_tokens (id, user_id, purpose, token_hash, expires_at)
    VALUES ($1, $2, $3, $4, $5)
  `, [randomUUID(), input.userId, input.purpose, input.tokenHash, input.expiresAt]);
  await client.query(`
    INSERT INTO email_outbox (id, user_id, recipient, template, payload)
    VALUES ($1, $2, $3, $4, $5::jsonb)
  `, [
    randomUUID(), input.userId, input.recipient, input.purpose,
    JSON.stringify({ encryptedToken: input.encryptedToken }),
  ]);
}

export async function queueAuthEmailForAddress(database: Database, input: {
  email: string;
  purpose: AuthEmailPurpose;
  tokenHash: string;
  encryptedToken: string;
  expiresAt: Date;
}): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ id: string; email: string }>(`
      SELECT id, email FROM users
      WHERE lower(email) = lower($1) AND status = 'active'
        AND ($2::text = 'verify_email' AND email_verified = false
          OR $2::text = 'reset_password' AND password_hash IS NOT NULL)
      FOR UPDATE
    `, [input.email, input.purpose]);
    const user = result.rows[0];
    if (user) await insertAuthEmail(client, { ...input, userId: user.id, recipient: user.email });
    await client.query("COMMIT");
    return Boolean(user);
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function consumeEmailVerification(database: Database, tokenHash: string): Promise<string | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ id: string; user_id: string }>(`
      SELECT id, user_id FROM auth_tokens
      WHERE token_hash = $1 AND purpose = 'verify_email' AND used_at IS NULL AND expires_at > now()
      FOR UPDATE
    `, [tokenHash]);
    const token = result.rows[0];
    if (!token) {
      await client.query("ROLLBACK");
      return null;
    }
    await client.query(`
      UPDATE auth_tokens SET used_at = now()
      WHERE user_id = $1 AND purpose = 'verify_email' AND used_at IS NULL
    `, [token.user_id]);
    await client.query("UPDATE users SET email_verified = true, updated_at = now() WHERE id = $1", [token.user_id]);
    // A user may request several verification messages before using one. Once verification wins,
    // the remaining unsent messages are intentionally obsolete, not delivery failures.
    await client.query(`
      UPDATE email_outbox
      SET status = 'canceled', locked_at = NULL, locked_by = NULL,
          last_error = 'superseded after successful email verification', updated_at = now()
      WHERE user_id = $1 AND template = 'verify_email' AND status = 'pending'
    `, [token.user_id]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id)
      VALUES ('user', $1, 'auth.email_verified', 'user', $1)
    `, [token.user_id]);
    await client.query("COMMIT");
    return token.user_id;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function consumePasswordReset(
  database: Database,
  tokenHash: string,
  passwordHash: string,
): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ id: string; user_id: string }>(`
      SELECT id, user_id FROM auth_tokens
      WHERE token_hash = $1 AND purpose = 'reset_password' AND used_at IS NULL AND expires_at > now()
      FOR UPDATE
    `, [tokenHash]);
    const token = result.rows[0];
    if (!token) {
      await client.query("ROLLBACK");
      return false;
    }
    await client.query(`
      UPDATE users SET password_hash = $2, email_verified = true, updated_at = now() WHERE id = $1
    `, [token.user_id, passwordHash]);
    await client.query("UPDATE auth_tokens SET used_at = now() WHERE user_id = $1 AND purpose = 'reset_password' AND used_at IS NULL", [token.user_id]);
    await client.query("UPDATE auth_sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL", [token.user_id]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id)
      VALUES ('user', $1, 'auth.password_reset', 'user', $1)
    `, [token.user_id]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function claimNextEmail(database: Database, workerId: string): Promise<ClaimedEmail | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(`
      UPDATE email_outbox SET status = 'failed', last_error = 'invalid encrypted token payload', updated_at = now()
      WHERE status = 'pending'
        AND (NOT (payload ? 'encryptedToken') OR jsonb_typeof(payload->'encryptedToken') <> 'string')
    `);
    const result = await client.query<{
      id: string; recipient: string; template: AuthEmailPurpose; payload: unknown; attempts: number;
    }>(`
      SELECT id, recipient, template, payload, attempts FROM email_outbox
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
      UPDATE email_outbox SET status = 'processing', attempts = attempts + 1,
        locked_at = now(), locked_by = $2, updated_at = now() WHERE id = $1
    `, [row.id, workerId]);
    await client.query("COMMIT");
    return { ...row, encryptedToken, attempts: row.attempts + 1 };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function confirmEmail(database: Database, id: string, providerMessageId: string): Promise<void> {
  await database.pool.query(`
    UPDATE email_outbox SET status = 'sent', sent_at = now(), provider_message_id = $2,
      locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
    WHERE id = $1 AND status = 'processing'
  `, [id, providerMessageId]);
}

export async function retryEmail(database: Database, job: ClaimedEmail, error: string): Promise<void> {
  const terminal = job.attempts >= 10;
  const delaySeconds = Math.min(3600, Math.max(10, 2 ** Math.min(job.attempts, 10)));
  await database.pool.query(`
    UPDATE email_outbox SET status = $2, next_attempt_at = now() + ($3 * interval '1 second'),
      locked_at = NULL, locked_by = NULL, last_error = $4, updated_at = now()
    WHERE id = $1 AND status = 'processing'
  `, [job.id, terminal ? "failed" : "pending", delaySeconds, error.slice(0, 2000)]);
}

export async function recoverStaleEmails(database: Database): Promise<number> {
  const result = await database.pool.query(`
    UPDATE email_outbox SET status = 'pending', locked_at = NULL, locked_by = NULL,
      next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}

function readEncryptedToken(payload: unknown): string {
  if (typeof payload !== "object" || payload === null || !("encryptedToken" in payload) ||
      typeof (payload as { encryptedToken: unknown }).encryptedToken !== "string") {
    throw new Error("email outbox payload has no encrypted token");
  }
  return (payload as { encryptedToken: string }).encryptedToken;
}
