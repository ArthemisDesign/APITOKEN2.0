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
  /** Ceiling delegated to the invited partner for their own future team. */
  teamOverrideMaxBps: number | null;
  /** Exact override the inviter receives from this invited partner. */
  parentOverrideBps: number | null;
  promoEnabled: boolean;
  promoMaxValueNano: bigint;
  promoMaxCount: number;
  referralDiscountBps: number;
  referralDiscountEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  teamInvitesEnabled: boolean;
  b2bCanDelegate: boolean;
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
  team_override_max_bps: number | null;
  parent_override_bps: number | null;
  promo_enabled: boolean;
  promo_max_value_nano: string;
  promo_max_count: number;
  referral_discount_bps: number;
  referral_discount_enabled: boolean;
  b2b_enabled: boolean;
  b2b_max_discount_bps: number;
  team_invites_enabled: boolean;
  b2b_can_delegate: boolean;
  expires_at: Date | null;
  consumed_at: Date | null;
  consumed_by_partner_id: string | null;
  created_at: Date;
}

const INVITE_COLUMNS = `
  id, partner_id, code, telegram_username, commission_bps, sub_commission_bps,
  team_override_max_bps, parent_override_bps,
  promo_enabled, promo_max_value_nano::text AS promo_max_value_nano, promo_max_count,
  referral_discount_bps, referral_discount_enabled,
  b2b_enabled, b2b_max_discount_bps, team_invites_enabled, b2b_can_delegate,
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
    teamOverrideMaxBps: row.team_override_max_bps,
    parentOverrideBps: row.parent_override_bps,
    promoEnabled: row.promo_enabled,
    promoMaxValueNano: BigInt(row.promo_max_value_nano),
    promoMaxCount: row.promo_max_count,
    referralDiscountBps: row.referral_discount_bps,
    referralDiscountEnabled: row.referral_discount_enabled,
    b2bEnabled: row.b2b_enabled,
    b2bMaxDiscountBps: row.b2b_max_discount_bps,
    teamInvitesEnabled: row.team_invites_enabled,
    b2bCanDelegate: row.b2b_can_delegate,
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
  teamOverrideMaxBps?: number | null;
  parentOverrideBps?: number | null;
  promoEnabled: boolean;
  promoMaxValueNano: bigint;
  promoMaxCount: number;
  referralDiscountBps: number;
  referralDiscountEnabled: boolean;
  /** B2B grant baked into the invite; the created partner holds it from the first sign-in. */
  b2bEnabled?: boolean;
  b2bMaxDiscountBps?: number;
  teamInvitesEnabled?: boolean;
  b2bCanDelegate?: boolean;
  /** Verified admin actor for a root invite; partner invites derive the actor from partnerId. */
  actorId?: string;
  expiresAt: Date;
}): Promise<PartnerInvite> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<InviteRow>(`
      INSERT INTO partner_invites (
        partner_id, code, telegram_username, commission_bps, sub_commission_bps,
        team_override_max_bps, parent_override_bps,
        promo_enabled, promo_max_value_nano, promo_max_count, referral_discount_bps,
        referral_discount_enabled, b2b_enabled, b2b_max_discount_bps,
        team_invites_enabled, b2b_can_delegate, expires_at
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
      RETURNING ${INVITE_COLUMNS}
    `, [
      input.partnerId, input.code, input.telegramUsername, input.commissionBps, input.subCommissionBps,
      input.teamOverrideMaxBps ?? null, input.parentOverrideBps ?? null,
      input.promoEnabled, input.promoMaxValueNano.toString(), input.promoMaxCount, input.referralDiscountBps,
      input.referralDiscountEnabled,
      input.b2bEnabled ?? false, input.b2bEnabled ? (input.b2bMaxDiscountBps ?? 0) : 0,
      input.teamInvitesEnabled ?? true,
      input.b2bEnabled ? (input.b2bCanDelegate ?? false) : false,
      input.expiresAt,
    ]);
    const invite = mapInvite(result.rows[0]!);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ($1, $2, 'partner.invite_created', 'partner_invite', $3, $4::jsonb)
    `, [
      input.partnerId === null ? "admin" : "partner",
      input.partnerId ?? input.actorId ?? "legacy-sales-admin",
      invite.id,
      JSON.stringify({
        parentPartnerId: input.partnerId,
        commissionBps: invite.commissionBps,
        subCommissionBps: invite.subCommissionBps,
        teamOverrideMaxBps: invite.teamOverrideMaxBps,
        parentOverrideBps: invite.parentOverrideBps,
        teamInvitesEnabled: invite.teamInvitesEnabled,
        b2bEnabled: invite.b2bEnabled,
        b2bMaxDiscountBps: invite.b2bMaxDiscountBps,
        b2bCanDelegate: invite.b2bCanDelegate,
      }),
    ]);
    await client.query("COMMIT");
    return invite;
  } catch (error) {
    await client.query("ROLLBACK");
    if (typeof error === "object" && error !== null && "code" in error && error.code === "23505") {
      throw new InviteCodeCollisionError("invite code collision");
    }
    throw error;
  } finally {
    client.release();
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
