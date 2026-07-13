import { randomUUID } from "node:crypto";
import type { Database } from "./client.js";

export interface AuthUser {
  id: string;
  email: string;
  emailVerified: boolean;
  status: "active" | "disabled";
  engineAccountStatus: "pending" | "active" | "error" | "disabled";
}

export interface PasswordUser extends AuthUser {
  passwordHash: string | null;
}

export class EmailAlreadyRegisteredError extends Error {}

export async function consumeAuthRateLimit(
  database: Database,
  input: { keyHash: string; maximum: number; windowSeconds: number },
): Promise<boolean> {
  const result = await database.pool.query<{ attempts: number }>(`
    INSERT INTO auth_rate_limits (key_hash, attempts, window_started_at)
    VALUES ($1, 1, now())
    ON CONFLICT (key_hash) DO UPDATE
    SET attempts = CASE
          WHEN auth_rate_limits.window_started_at < now() - ($2 * interval '1 second') THEN 1
          ELSE auth_rate_limits.attempts + 1
        END,
        window_started_at = CASE
          WHEN auth_rate_limits.window_started_at < now() - ($2 * interval '1 second') THEN now()
          ELSE auth_rate_limits.window_started_at
        END,
        updated_at = now()
    RETURNING attempts
  `, [input.keyHash, input.windowSeconds]);
  return (result.rows[0]?.attempts ?? input.maximum + 1) <= input.maximum;
}

export async function clearAuthRateLimit(database: Database, keyHashes: readonly string[]): Promise<void> {
  if (keyHashes.length === 0) return;
  await database.pool.query("DELETE FROM auth_rate_limits WHERE key_hash = ANY($1::text[])", [keyHashes]);
}

export async function createEmailUser(database: Database, email: string, passwordHash: string): Promise<AuthUser> {
  const client = await database.pool.connect();
  const userId = randomUUID();
  try {
    await client.query("BEGIN");
    await client.query(`
      INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)
    `, [userId, email, passwordHash]);
    await client.query(`
      INSERT INTO engine_accounts (id, user_id, status) VALUES ($1, $2, 'pending')
    `, [randomUUID(), userId]);
    await client.query(`
      INSERT INTO email_outbox (id, user_id, recipient, template, payload)
      VALUES ($1, $2, $3, 'verify_email', '{}'::jsonb)
    `, [randomUUID(), userId, email]);
    await client.query("COMMIT");
    return { id: userId, email, emailVerified: false, status: "active", engineAccountStatus: "pending" };
  } catch (error) {
    await client.query("ROLLBACK");
    if (isUniqueViolation(error)) throw new EmailAlreadyRegisteredError("email is already registered");
    throw error;
  } finally {
    client.release();
  }
}

export async function completeEngineAccount(database: Database, userId: string, engineAccountId: string): Promise<void> {
  await database.pool.query(`
    UPDATE engine_accounts
    SET engine_account_id = $2, status = 'active', last_error = NULL, updated_at = now()
    WHERE user_id = $1 AND (status IN ('pending', 'error') OR engine_account_id IS NULL)
  `, [userId, engineAccountId]);
}

export async function failEngineAccount(database: Database, userId: string, error: string): Promise<void> {
  await database.pool.query(`
    UPDATE engine_accounts SET status = 'error', last_error = $2, updated_at = now() WHERE user_id = $1
  `, [userId, error.slice(0, 1000)]);
}

export async function findPasswordUser(database: Database, email: string): Promise<PasswordUser | null> {
  const result = await database.pool.query<UserRow>(`
    SELECT u.id, u.email, u.email_verified, u.password_hash, u.status,
           COALESCE(ea.status, 'pending') AS engine_account_status
    FROM users u LEFT JOIN engine_accounts ea ON ea.user_id = u.id
    WHERE lower(u.email) = lower($1)
  `, [email]);
  return result.rows[0] ? mapPasswordUser(result.rows[0]) : null;
}

export async function getAuthUser(database: Database, userId: string): Promise<AuthUser | null> {
  const result = await database.pool.query<UserRow>(`
    SELECT u.id, u.email, u.email_verified, u.password_hash, u.status,
           COALESCE(ea.status, 'pending') AS engine_account_status
    FROM users u LEFT JOIN engine_accounts ea ON ea.user_id = u.id WHERE u.id = $1
  `, [userId]);
  return result.rows[0] ? withoutPassword(mapPasswordUser(result.rows[0])) : null;
}

export async function createAuthSession(database: Database, input: {
  userId: string; tokenHash: string; expiresAt: Date; userAgent: string | null; ipAddress: string | null;
}): Promise<string> {
  const id = randomUUID();
  await database.pool.query(`
    INSERT INTO auth_sessions (id, user_id, token_hash, expires_at, user_agent, ip_address)
    VALUES ($1, $2, $3, $4, $5, $6)
  `, [id, input.userId, input.tokenHash, input.expiresAt, input.userAgent, input.ipAddress]);
  return id;
}

export async function resolveAuthSession(database: Database, tokenHash: string): Promise<{ sessionId: string; user: AuthUser } | null> {
  const result = await database.pool.query<UserRow & { session_id: string }>(`
    SELECT s.id AS session_id, u.id, u.email, u.email_verified, u.password_hash, u.status,
           COALESCE(ea.status, 'pending') AS engine_account_status
    FROM auth_sessions s
    JOIN users u ON u.id = s.user_id
    LEFT JOIN engine_accounts ea ON ea.user_id = u.id
    WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now() AND u.status = 'active'
  `, [tokenHash]);
  const row = result.rows[0];
  if (!row) return null;
  return { sessionId: row.session_id, user: withoutPassword(mapPasswordUser(row)) };
}

export async function revokeAuthSession(database: Database, sessionId: string, userId: string): Promise<void> {
  await database.pool.query(`
    UPDATE auth_sessions SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
  `, [sessionId, userId]);
}

export async function linkExternalIdentity(database: Database, input: {
  userId: string; provider: string; subject: string; email: string | null; emailVerified: boolean; metadata: unknown;
}): Promise<void> {
  await database.pool.query(`
    INSERT INTO auth_identities (id, user_id, provider, subject, email, email_verified, metadata)
    VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
  `, [randomUUID(), input.userId, input.provider, input.subject, input.email, input.emailVerified, JSON.stringify(input.metadata)]);
}

interface UserRow {
  id: string;
  email: string;
  email_verified: boolean;
  password_hash: string | null;
  status: "active" | "disabled";
  engine_account_status: "pending" | "active" | "error" | "disabled";
}

function mapPasswordUser(row: UserRow): PasswordUser {
  return {
    id: row.id,
    email: row.email,
    emailVerified: row.email_verified,
    passwordHash: row.password_hash,
    status: row.status,
    engineAccountStatus: row.engine_account_status,
  };
}

function withoutPassword(user: PasswordUser): AuthUser {
  const { passwordHash: _passwordHash, ...safe } = user;
  return safe;
}

function isUniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "23505";
}
