import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";

export type PartnerStatus = "active" | "suspended" | "pending";
export type PartnerAuthPurpose = "verify_email" | "reset_password";

export interface Partner {
  id: string;
  email: string | null;
  displayName: string | null;
  // Telegram identity (онбординг через Telegram Login). id хранится строкой (pg bigint).
  telegramId: string | null;
  telegramUsername: string | null;
  telegramPhotoUrl: string | null;
  status: PartnerStatus;
  emailVerified: boolean;
  referralCode: string;
  parentPartnerId: string | null;
  commissionBps: number;
  subCommissionBps: number;
  payoutMethod: string | null;
  payoutDetails: unknown;
  promoEnabled: boolean;
  promoMaxValueNano: bigint;
  promoMaxCount: number;
  referralDiscountBps: number;
  referralDiscountEnabled: boolean;
  createdAt: Date;
}

export interface PasswordPartner extends Partner {
  passwordHash: string | null;
}

export class EmailAlreadyRegisteredError extends Error {}
export class ReferralCodeCollisionError extends Error {}
export class InvalidInviteError extends Error {}

interface PartnerRow {
  id: string;
  email: string | null;
  display_name: string | null;
  password_hash: string | null;
  telegram_id: string | null;
  telegram_username: string | null;
  telegram_photo_url: string | null;
  status: PartnerStatus;
  email_verified: boolean;
  referral_code: string;
  parent_partner_id: string | null;
  commission_bps: number;
  sub_commission_bps: number;
  payout_method: string | null;
  payout_details: unknown;
  promo_enabled: boolean;
  promo_max_value_nano: string;
  promo_max_count: number;
  referral_discount_bps: number;
  referral_discount_enabled: boolean;
  created_at: Date;
}

const PARTNER_COLUMNS = `
  id, email, display_name, password_hash, telegram_id, telegram_username, telegram_photo_url,
  status, email_verified, referral_code,
  parent_partner_id, commission_bps, sub_commission_bps, payout_method, payout_details,
  promo_enabled, promo_max_value_nano::text AS promo_max_value_nano, promo_max_count,
  referral_discount_bps, referral_discount_enabled, created_at
`;

function mapPartner(row: PartnerRow): PasswordPartner {
  return {
    id: row.id,
    email: row.email,
    displayName: row.display_name,
    passwordHash: row.password_hash,
    telegramId: row.telegram_id,
    telegramUsername: row.telegram_username,
    telegramPhotoUrl: row.telegram_photo_url,
    status: row.status,
    emailVerified: row.email_verified,
    referralCode: row.referral_code,
    parentPartnerId: row.parent_partner_id,
    commissionBps: row.commission_bps,
    subCommissionBps: row.sub_commission_bps,
    payoutMethod: row.payout_method,
    payoutDetails: row.payout_details,
    promoEnabled: row.promo_enabled,
    promoMaxValueNano: BigInt(row.promo_max_value_nano),
    promoMaxCount: row.promo_max_count,
    referralDiscountBps: row.referral_discount_bps,
    referralDiscountEnabled: row.referral_discount_enabled,
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
  id: string; partnerId: string | null; commissionBps: number | null;
  subCommissionBps: number | null; telegramUsername: string | null;
  promoEnabled: boolean; promoMaxValueNano: string; promoMaxCount: number;
  referralDiscountBps: number; referralDiscountEnabled: boolean;
}> {
  const result = await client.query<{
    id: string; partner_id: string | null; commission_bps: number | null;
    sub_commission_bps: number | null; telegram_username: string | null;
    promo_enabled: boolean; promo_max_value_nano: string; promo_max_count: number;
    referral_discount_bps: number; referral_discount_enabled: boolean;
  }>(`
    SELECT id, partner_id, commission_bps, sub_commission_bps, telegram_username,
           promo_enabled, promo_max_value_nano::text AS promo_max_value_nano, promo_max_count,
           referral_discount_bps, referral_discount_enabled
    FROM partner_invites
    WHERE code = $1 AND consumed_at IS NULL AND (expires_at IS NULL OR expires_at > now())
    FOR UPDATE
  `, [code]);
  const row = result.rows[0];
  if (!row) throw new InvalidInviteError("invite code is invalid or expired");
  return {
    id: row.id,
    partnerId: row.partner_id,
    commissionBps: row.commission_bps,
    subCommissionBps: row.sub_commission_bps,
    telegramUsername: row.telegram_username,
    promoEnabled: row.promo_enabled,
    promoMaxValueNano: row.promo_max_value_nano,
    promoMaxCount: row.promo_max_count,
    referralDiscountBps: row.referral_discount_bps,
    referralDiscountEnabled: row.referral_discount_enabled,
  };
}

export class TelegramAlreadyRegisteredError extends Error {}

export async function findTelegramPartner(database: SalesDatabase, telegramId: string): Promise<Partner | null> {
  const result = await database.pool.query<PartnerRow>(`
    SELECT ${PARTNER_COLUMNS} FROM partners WHERE telegram_id = $1
  `, [telegramId]);
  return result.rows[0] ? withoutPassword(mapPartner(result.rows[0])) : null;
}

/**
 * Онбординг через Telegram: партнёр создаётся ТОЛЬКО по валидному инвайту, чей
 * telegram_username (если задан) совпал с юзернеймом вошедшего. Сразу active.
 */
export async function createTelegramPartner(database: SalesDatabase, input: {
  telegramId: string;
  telegramUsername: string | null;
  telegramPhotoUrl: string | null;
  displayName: string | null;
  referralCode: string;
  inviteCode: string;
  defaultCommissionBps: number;
  defaultSubCommissionBps: number;
}): Promise<Partner> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const invite = await lockInvite(client, input.inviteCode);
    if (invite.telegramUsername) {
      const actual = (input.telegramUsername ?? "").toLowerCase();
      if (actual !== invite.telegramUsername.toLowerCase()) {
        throw new InvalidInviteError("this invite is issued for a different telegram account");
      }
    }
    const result = await client.query<PartnerRow>(`
      INSERT INTO partners (
        telegram_id, telegram_username, telegram_photo_url, display_name, status,
        referral_code, parent_partner_id, commission_bps, sub_commission_bps,
        promo_enabled, promo_max_value_nano, promo_max_count, referral_discount_bps, referral_discount_enabled
      )
      VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, $8, $9, $10, $11, $12, $13)
      RETURNING ${PARTNER_COLUMNS}
    `, [
      input.telegramId, input.telegramUsername, input.telegramPhotoUrl, input.displayName,
      input.referralCode, invite.partnerId,
      invite.commissionBps ?? input.defaultCommissionBps,
      invite.subCommissionBps ?? input.defaultSubCommissionBps,
      invite.promoEnabled, invite.promoMaxValueNano, invite.promoMaxCount,
      invite.referralDiscountBps, invite.referralDiscountEnabled,
    ]);
    const partner = mapPartner(result.rows[0]!);
    await client.query(`
      UPDATE partner_invites SET consumed_at = now(), consumed_by_partner_id = $2
      WHERE id = $1 AND consumed_at IS NULL
    `, [invite.id, partner.id]);
    await client.query("COMMIT");
    return withoutPassword(partner);
  } catch (error) {
    await client.query("ROLLBACK");
    if (isUniqueViolation(error)) {
      if (constraintName(error) === "partners_referral_code_uidx") {
        throw new ReferralCodeCollisionError("referral code collision");
      }
      throw new TelegramAlreadyRegisteredError("telegram account is already registered");
    }
    throw error;
  } finally {
    client.release();
  }
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
    SELECT s.id AS session_id, p.id, p.email, p.display_name, p.password_hash,
           p.telegram_id, p.telegram_username, p.telegram_photo_url, p.status,
           p.email_verified, p.referral_code, p.parent_partner_id, p.commission_bps,
           p.sub_commission_bps, p.payout_method, p.payout_details,
           p.promo_enabled, p.promo_max_value_nano::text AS promo_max_value_nano, p.promo_max_count,
           p.referral_discount_bps, p.referral_discount_enabled, p.created_at
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
