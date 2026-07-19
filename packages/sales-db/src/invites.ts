import type { SalesDatabase } from "./client.js";

export interface PartnerInvite {
  id: string;
  // NULL — корневой инвайт из админки (созданный партнёр не имеет родителя).
  partnerId: string | null;
  code: string;
  // Нормализованный telegram-юзернейм (без @, lower). Регистрация по инвайту — только
  // если юзернейм вошедшего через Telegram совпал.
  telegramUsername: string | null;
  commissionBps: number | null;
  subCommissionBps: number | null;
  promoEnabled: boolean;
  promoMaxValueNano: bigint;
  promoMaxCount: number;
  referralDiscountBps: number;
  referralDiscountEnabled: boolean;
  expiresAt: Date | null;
  consumedAt: Date | null;
  consumedByPartnerId: string | null;
  createdAt: Date;
}

export class InviteCodeCollisionError extends Error {}

interface InviteRow {
  id: string;
  partner_id: string | null;
  code: string;
  telegram_username: string | null;
  commission_bps: number | null;
  sub_commission_bps: number | null;
  promo_enabled: boolean;
  promo_max_value_nano: string;
  promo_max_count: number;
  referral_discount_bps: number;
  referral_discount_enabled: boolean;
  expires_at: Date | null;
  consumed_at: Date | null;
  consumed_by_partner_id: string | null;
  created_at: Date;
}

const INVITE_COLUMNS = `
  id, partner_id, code, telegram_username, commission_bps, sub_commission_bps,
  promo_enabled, promo_max_value_nano::text AS promo_max_value_nano, promo_max_count,
  referral_discount_bps, referral_discount_enabled,
  expires_at, consumed_at, consumed_by_partner_id, created_at
`;

function mapInvite(row: InviteRow): PartnerInvite {
  return {
    id: row.id,
    partnerId: row.partner_id,
    code: row.code,
    telegramUsername: row.telegram_username,
    commissionBps: row.commission_bps,
    subCommissionBps: row.sub_commission_bps,
    promoEnabled: row.promo_enabled,
    promoMaxValueNano: BigInt(row.promo_max_value_nano),
    promoMaxCount: row.promo_max_count,
    referralDiscountBps: row.referral_discount_bps,
    referralDiscountEnabled: row.referral_discount_enabled,
    expiresAt: row.expires_at,
    consumedAt: row.consumed_at,
    consumedByPartnerId: row.consumed_by_partner_id,
    createdAt: row.created_at,
  };
}

export async function createPartnerInvite(database: SalesDatabase, input: {
  partnerId: string | null;
  code: string;
  telegramUsername: string | null;
  commissionBps: number | null;
  subCommissionBps: number | null;
  promoEnabled: boolean;
  promoMaxValueNano: bigint;
  promoMaxCount: number;
  referralDiscountBps: number;
  referralDiscountEnabled: boolean;
  expiresAt: Date;
}): Promise<PartnerInvite> {
  try {
    const result = await database.pool.query<InviteRow>(`
      INSERT INTO partner_invites (
        partner_id, code, telegram_username, commission_bps, sub_commission_bps,
        promo_enabled, promo_max_value_nano, promo_max_count, referral_discount_bps,
        referral_discount_enabled, expires_at
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
      RETURNING ${INVITE_COLUMNS}
    `, [
      input.partnerId, input.code, input.telegramUsername, input.commissionBps, input.subCommissionBps,
      input.promoEnabled, input.promoMaxValueNano.toString(), input.promoMaxCount, input.referralDiscountBps,
      input.referralDiscountEnabled, input.expiresAt,
    ]);
    return mapInvite(result.rows[0]!);
  } catch (error) {
    if (typeof error === "object" && error !== null && "code" in error && error.code === "23505") {
      throw new InviteCodeCollisionError("invite code collision");
    }
    throw error;
  }
}

export async function listPartnerInvites(database: SalesDatabase, partnerId: string): Promise<PartnerInvite[]> {
  const result = await database.pool.query<InviteRow>(`
    SELECT ${INVITE_COLUMNS}
    FROM partner_invites WHERE partner_id = $1 ORDER BY created_at DESC
  `, [partnerId]);
  return result.rows.map(mapInvite);
}

/** Корневые инвайты админки (partner_id IS NULL). */
export async function listRootInvites(database: SalesDatabase): Promise<PartnerInvite[]> {
  const result = await database.pool.query<InviteRow>(`
    SELECT ${INVITE_COLUMNS}
    FROM partner_invites WHERE partner_id IS NULL ORDER BY created_at DESC
  `);
  return result.rows.map(mapInvite);
}

/** Активный инвайт, выписанный на данный telegram-юзернейм (для входа без ссылки). */
export async function getActiveInviteByTelegramUsername(
  database: SalesDatabase,
  telegramUsername: string,
): Promise<PartnerInvite | null> {
  const result = await database.pool.query<InviteRow>(`
    SELECT ${INVITE_COLUMNS}
    FROM partner_invites
    WHERE lower(telegram_username) = lower($1)
      AND consumed_at IS NULL AND (expires_at IS NULL OR expires_at > now())
    ORDER BY created_at DESC
    LIMIT 1
  `, [telegramUsername]);
  return result.rows[0] ? mapInvite(result.rows[0]) : null;
}

/** Публичная проверка инвайта (для страницы регистрации). */
export async function getActiveInviteByCode(database: SalesDatabase, code: string): Promise<PartnerInvite | null> {
  const result = await database.pool.query<InviteRow>(`
    SELECT ${INVITE_COLUMNS}
    FROM partner_invites
    WHERE code = $1 AND consumed_at IS NULL AND (expires_at IS NULL OR expires_at > now())
  `, [code]);
  return result.rows[0] ? mapInvite(result.rows[0]) : null;
}
