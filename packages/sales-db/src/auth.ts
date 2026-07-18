import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";
import { insertPartnerEmail } from "./outbox.js";

export type PartnerStatus = "active" | "suspended" | "pending";
export type PartnerAuthPurpose = "verify_email" | "reset_password";

export interface Partner {
  id: string;
  email: string;
  displayName: string | null;
  status: PartnerStatus;
  emailVerified: boolean;
  referralCode: string;
  parentPartnerId: string | null;
  commissionBps: number;
  subCommissionBps: number;
  payoutMethod: string | null;
  payoutDetails: unknown;
  createdAt: Date;
}

export interface PasswordPartner extends Partner {
  passwordHash: string;
}

export class EmailAlreadyRegisteredError extends Error {}
export class ReferralCodeCollisionError extends Error {}
export class InvalidInviteError extends Error {}

interface PartnerRow {
  id: string;
  email: string;
  display_name: string | null;
  password_hash: string;
  status: PartnerStatus;
  email_verified: boolean;
  referral_code: string;
  parent_partner_id: string | null;
  commission_bps: number;
  sub_commission_bps: number;
  payout_method: string | null;
  payout_details: unknown;
  created_at: Date;
}

const PARTNER_COLUMNS = `
  id, email, display_name, password_hash, status, email_verified, referral_code,
  parent_partner_id, commission_bps, sub_commission_bps, payout_method, payout_details, created_at
`;

function mapPartner(row: PartnerRow): PasswordPartner {
  return {
    id: row.id,
    email: row.email,
    displayName: row.display_name,
    passwordHash: row.password_hash,
    status: row.status,
    emailVerified: row.email_verified,
    referralCode: row.referral_code,
    parentPartnerId: row.parent_partner_id,
    commissionBps: row.commission_bps,
    subCommissionBps: row.sub_commission_bps,
    payoutMethod: row.payout_method,
    payoutDetails: row.payout_details,
    createdAt: row.created_at,
  };
}

function withoutPassword(partner: PasswordPartner): Partner {
  const { passwordHash: _passwordHash, ...safe } = partner;
  return safe;
}

export async function consumePartnerRateLimit(
  database: SalesDatabase,
  input: { keyHash: string; maximum: number; windowSeconds: number },
): Promise<boolean> {
  const result = await database.pool.query<{ attempts: number }>(`
    INSERT INTO partner_rate_limits (key_hash, attempts, window_started_at)
    VALUES ($1, 1, now())
    ON CONFLICT (key_hash) DO UPDATE
    SET attempts = CASE
          WHEN partner_rate_limits.window_started_at < now() - ($2 * interval '1 second') THEN 1
          ELSE partner_rate_limits.attempts + 1
        END,
        window_started_at = CASE
          WHEN partner_rate_limits.window_started_at < now() - ($2 * interval '1 second') THEN now()
          ELSE partner_rate_limits.window_started_at
        END,
        updated_at = now()
    RETURNING attempts
  `, [input.keyHash, input.windowSeconds]);
  return (result.rows[0]?.attempts ?? input.maximum + 1) <= input.maximum;
}

export async function clearPartnerRateLimit(database: SalesDatabase, keyHashes: readonly string[]): Promise<void> {
  if (keyHashes.length === 0) return;
  await database.pool.query("DELETE FROM partner_rate_limits WHERE key_hash = ANY($1::text[])", [keyHashes]);
}

async function lockInvite(client: PoolClient, code: string): Promise<{
  id: string; partnerId: string; commissionBps: number | null;
}> {
  const result = await client.query<{ id: string; partner_id: string; commission_bps: number | null }>(`
    SELECT id, partner_id, commission_bps FROM partner_invites
    WHERE code = $1 AND consumed_at IS NULL AND (expires_at IS NULL OR expires_at > now())
    FOR UPDATE
  `, [code]);
  const row = result.rows[0];
  if (!row) throw new InvalidInviteError("invite code is invalid or expired");
  return { id: row.id, partnerId: row.partner_id, commissionBps: row.commission_bps };
}

export async function createPartner(database: SalesDatabase, input: {
  email: string;
  passwordHash: string;
  displayName: string | null;
  referralCode: string;
  inviteCode: string | null;
  commissionBps: number;
  subCommissionBps: number;
  verification: { tokenHash: string; encryptedToken: string; expiresAt: Date };
}): Promise<Partner> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const invite = input.inviteCode ? await lockInvite(client, input.inviteCode) : null;
    const commissionBps = invite?.commissionBps ?? input.commissionBps;
    const result = await client.query<PartnerRow>(`
      INSERT INTO partners (email, display_name, password_hash, referral_code, parent_partner_id, commission_bps, sub_commission_bps)
      VALUES ($1, $2, $3, $4, $5, $6, $7)
      RETURNING ${PARTNER_COLUMNS}
    `, [
      input.email, input.displayName, input.passwordHash, input.referralCode,
      invite?.partnerId ?? null, commissionBps, input.subCommissionBps,
    ]);
    const partner = mapPartner(result.rows[0]!);
    if (invite) {
      await client.query(`
        UPDATE partner_invites SET consumed_at = now(), consumed_by_partner_id = $2
        WHERE id = $1 AND consumed_at IS NULL
      `, [invite.id, partner.id]);
    }
    await insertPartnerEmail(client, {
      partnerId: partner.id,
      recipient: partner.email,
      purpose: "verify_email",
      ...input.verification,
    });
    await client.query("COMMIT");
    return withoutPassword(partner);
  } catch (error) {
    await client.query("ROLLBACK");
    if (isUniqueViolation(error)) {
      if (constraintName(error) === "partners_referral_code_uidx") {
        throw new ReferralCodeCollisionError("referral code collision");
      }
      throw new EmailAlreadyRegisteredError("email is already registered");
    }
    throw error;
  } finally {
    client.release();
  }
}

export async function findPasswordPartner(database: SalesDatabase, email: string): Promise<PasswordPartner | null> {
  const result = await database.pool.query<PartnerRow>(`
    SELECT ${PARTNER_COLUMNS} FROM partners WHERE lower(email) = lower($1)
  `, [email]);
  return result.rows[0] ? mapPartner(result.rows[0]) : null;
}

export async function getPartner(database: SalesDatabase, partnerId: string): Promise<Partner | null> {
  const result = await database.pool.query<PartnerRow>(`
    SELECT ${PARTNER_COLUMNS} FROM partners WHERE id = $1
  `, [partnerId]);
  return result.rows[0] ? withoutPassword(mapPartner(result.rows[0])) : null;
}

export async function findPartnerByReferralCode(database: SalesDatabase, referralCode: string): Promise<Partner | null> {
  const result = await database.pool.query<PartnerRow>(`
    SELECT ${PARTNER_COLUMNS} FROM partners WHERE referral_code = $1
  `, [referralCode]);
  return result.rows[0] ? withoutPassword(mapPartner(result.rows[0])) : null;
}

export async function createPartnerSession(database: SalesDatabase, input: {
  partnerId: string; tokenHash: string; expiresAt: Date; userAgent: string | null; ipAddress: string | null;
}): Promise<string> {
  const result = await database.pool.query<{ id: string }>(`
    INSERT INTO partner_sessions (partner_id, token_hash, expires_at, user_agent, ip_address)
    VALUES ($1, $2, $3, $4, $5)
    RETURNING id
  `, [input.partnerId, input.tokenHash, input.expiresAt, input.userAgent, input.ipAddress]);
  return result.rows[0]!.id;
}

export async function resolvePartnerSession(
  database: SalesDatabase,
  tokenHash: string,
): Promise<{ sessionId: string; partner: Partner } | null> {
  const result = await database.pool.query<PartnerRow & { session_id: string }>(`
    SELECT s.id AS session_id, p.id, p.email, p.display_name, p.password_hash, p.status,
           p.email_verified, p.referral_code, p.parent_partner_id, p.commission_bps,
           p.sub_commission_bps, p.payout_method, p.payout_details, p.created_at
    FROM partner_sessions s
    JOIN partners p ON p.id = s.partner_id
    WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now() AND p.status = 'active'
  `, [tokenHash]);
  const row = result.rows[0];
  if (!row) return null;
  await database.pool.query("UPDATE partner_sessions SET last_seen_at = now() WHERE id = $1", [row.session_id]);
  return { sessionId: row.session_id, partner: withoutPassword(mapPartner(row)) };
}

export async function revokePartnerSession(database: SalesDatabase, sessionId: string, partnerId: string): Promise<void> {
  await database.pool.query(`
    UPDATE partner_sessions SET revoked_at = now() WHERE id = $1 AND partner_id = $2 AND revoked_at IS NULL
  `, [sessionId, partnerId]);
}

export async function queuePartnerEmailForAddress(database: SalesDatabase, input: {
  email: string;
  purpose: PartnerAuthPurpose;
  tokenHash: string;
  encryptedToken: string;
  expiresAt: Date;
}): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ id: string; email: string }>(`
      SELECT id, email FROM partners
      WHERE lower(email) = lower($1) AND status <> 'suspended'
        AND ($2::text = 'reset_password' OR email_verified = false)
      FOR UPDATE
    `, [input.email, input.purpose]);
    const partner = result.rows[0];
    if (partner) {
      await insertPartnerEmail(client, {
        partnerId: partner.id,
        recipient: partner.email,
        purpose: input.purpose,
        tokenHash: input.tokenHash,
        encryptedToken: input.encryptedToken,
        expiresAt: input.expiresAt,
      });
    }
    await client.query("COMMIT");
    return Boolean(partner);
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function consumePartnerEmailVerification(database: SalesDatabase, tokenHash: string): Promise<string | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ partner_id: string }>(`
      SELECT partner_id FROM partner_auth_tokens
      WHERE token_hash = $1 AND purpose = 'verify_email' AND used_at IS NULL AND expires_at > now()
      FOR UPDATE
    `, [tokenHash]);
    const token = result.rows[0];
    if (!token) {
      await client.query("ROLLBACK");
      return null;
    }
    await client.query(`
      UPDATE partner_auth_tokens SET used_at = now()
      WHERE partner_id = $1 AND purpose = 'verify_email' AND used_at IS NULL
    `, [token.partner_id]);
    await client.query(`
      UPDATE partners
      SET email_verified = true,
          status = CASE WHEN status = 'pending' THEN 'active'::partner_status ELSE status END,
          updated_at = now()
      WHERE id = $1
    `, [token.partner_id]);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id)
      VALUES ('partner', $1, 'auth.email_verified', 'partner', $1)
    `, [token.partner_id]);
    await client.query("COMMIT");
    return token.partner_id;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function consumePartnerPasswordReset(
  database: SalesDatabase,
  tokenHash: string,
  passwordHash: string,
): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ partner_id: string }>(`
      SELECT partner_id FROM partner_auth_tokens
      WHERE token_hash = $1 AND purpose = 'reset_password' AND used_at IS NULL AND expires_at > now()
      FOR UPDATE
    `, [tokenHash]);
    const token = result.rows[0];
    if (!token) {
      await client.query("ROLLBACK");
      return false;
    }
    await client.query(`
      UPDATE partners SET password_hash = $2, email_verified = true,
        status = CASE WHEN status = 'pending' THEN 'active'::partner_status ELSE status END,
        updated_at = now()
      WHERE id = $1
    `, [token.partner_id, passwordHash]);
    await client.query(`
      UPDATE partner_auth_tokens SET used_at = now()
      WHERE partner_id = $1 AND purpose = 'reset_password' AND used_at IS NULL
    `, [token.partner_id]);
    await client.query(`
      UPDATE partner_sessions SET revoked_at = now() WHERE partner_id = $1 AND revoked_at IS NULL
    `, [token.partner_id]);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id)
      VALUES ('partner', $1, 'auth.password_reset', 'partner', $1)
    `, [token.partner_id]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function updatePartnerSettings(database: SalesDatabase, partnerId: string, input: {
  displayName?: string;
  payoutMethod?: string;
  payoutDetails?: unknown;
}): Promise<Partner | null> {
  const result = await database.pool.query<PartnerRow>(`
    UPDATE partners
    SET display_name = COALESCE($2, display_name),
        payout_method = COALESCE($3, payout_method),
        payout_details = COALESCE($4::jsonb, payout_details),
        updated_at = now()
    WHERE id = $1 AND status = 'active'
    RETURNING ${PARTNER_COLUMNS}
  `, [
    partnerId,
    input.displayName ?? null,
    input.payoutMethod ?? null,
    input.payoutDetails === undefined ? null : JSON.stringify(input.payoutDetails),
  ]);
  return result.rows[0] ? withoutPassword(mapPartner(result.rows[0])) : null;
}

function isUniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "23505";
}

function constraintName(error: unknown): string | null {
  if (typeof error === "object" && error !== null && "constraint" in error) {
    const constraint = (error as { constraint: unknown }).constraint;
    return typeof constraint === "string" ? constraint : null;
  }
  return null;
}
