import type { SalesDatabase } from "./client.js";

export type PromoCodeStatus = "active" | "redeemed" | "disabled";

export interface PromoCode {
  id: string;
  partnerId: string;
  code: string;
  valueNano: bigint;
  status: PromoCodeStatus;
  redeemedByCommerceUserId: string | null;
  redeemedAt: Date | null;
  createdAt: Date;
}

export class PromoNotAllowedError extends Error {}
export class PromoLimitError extends Error {}
export class PromoCodeCollisionError extends Error {}
export class PromoNotFoundError extends Error {}
export class PromoAlreadyRedeemedError extends Error {}
export class UserAlreadyRedeemedError extends Error {}

interface PromoRow {
  id: string;
  partner_id: string;
  code: string;
  value_nano: string;
  status: PromoCodeStatus;
  redeemed_by_commerce_user_id: string | null;
  redeemed_at: Date | null;
  created_at: Date;
}

const PROMO_COLUMNS = `
  id, partner_id, code, value_nano::text AS value_nano, status,
  redeemed_by_commerce_user_id, redeemed_at, created_at
`;

function mapPromo(row: PromoRow): PromoCode {
  return {
    id: row.id,
    partnerId: row.partner_id,
    code: row.code,
    valueNano: BigInt(row.value_nano),
    status: row.status,
    redeemedByCommerceUserId: row.redeemed_by_commerce_user_id,
    redeemedAt: row.redeemed_at,
    createdAt: row.created_at,
  };
}

/**
 * Партнёр создаёт промокод. Проверяет право (`promo_enabled`), лимит номинала
 * (`promo_max_value_nano`) и лимит количества (`promo_max_count`) под блокировкой строки партнёра.
 */
export async function createPromoCode(database: SalesDatabase, input: {
  partnerId: string;
  code: string;
  valueNano: bigint;
}): Promise<PromoCode> {
  if (input.valueNano <= 0n) throw new PromoLimitError("promo value must be positive");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const partner = await client.query<{
      promo_enabled: boolean; promo_max_value_nano: string; promo_max_count: number; status: string;
    }>(
      `SELECT promo_enabled, promo_max_value_nano::text AS promo_max_value_nano, promo_max_count, status
       FROM partners WHERE id = $1 FOR UPDATE`,
      [input.partnerId],
    );
    const row = partner.rows[0];
    if (!row || row.status !== "active" || !row.promo_enabled) {
      await client.query("ROLLBACK");
      throw new PromoNotAllowedError("promo codes are not enabled for this partner");
    }
    const maxValue = BigInt(row.promo_max_value_nano);
    if (input.valueNano > maxValue) {
      await client.query("ROLLBACK");
      throw new PromoLimitError("promo value exceeds the allowed maximum");
    }
    const count = await client.query<{ n: string }>(
      "SELECT count(*)::text AS n FROM promo_codes WHERE partner_id = $1",
      [input.partnerId],
    );
    if (Number(count.rows[0]!.n) >= row.promo_max_count) {
      await client.query("ROLLBACK");
      throw new PromoLimitError("promo code count limit reached");
    }
    let inserted;
    try {
      inserted = await client.query<PromoRow>(
        `INSERT INTO promo_codes (partner_id, code, value_nano) VALUES ($1, $2, $3)
         RETURNING ${PROMO_COLUMNS}`,
        [input.partnerId, input.code, input.valueNano.toString()],
      );
    } catch (error) {
      await client.query("ROLLBACK");
      if (isUniqueViolation(error)) throw new PromoCodeCollisionError("promo code already exists");
      throw error;
    }
    await client.query(
      `INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
       VALUES ('partner', $1, 'promo.created', 'promo_code', $2, $3::jsonb)`,
      [input.partnerId, inserted.rows[0]!.id, JSON.stringify({ code: input.code, valueNano: input.valueNano.toString() })],
    );
    await client.query("COMMIT");
    return mapPromo(inserted.rows[0]!);
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    client.release();
  }
}

export async function listPartnerPromoCodes(database: SalesDatabase, partnerId: string): Promise<PromoCode[]> {
  const result = await database.pool.query<PromoRow>(
    `SELECT ${PROMO_COLUMNS} FROM promo_codes WHERE partner_id = $1 ORDER BY created_at DESC`,
    [partnerId],
  );
  return result.rows.map(mapPromo);
}

export async function disablePromoCode(database: SalesDatabase, partnerId: string, promoId: string): Promise<boolean> {
  const result = await database.pool.query(
    `UPDATE promo_codes SET status = 'disabled'
     WHERE id = $1 AND partner_id = $2 AND status = 'active'`,
    [promoId, partnerId],
  );
  return (result.rowCount ?? 0) > 0;
}

export interface PromoRedemption {
  valueNano: bigint;
  partnerId: string;
  referralCode: string;
  redemptionRef: string;
  alreadyRedeemed: boolean;
}

/**
 * Атомарно погашает промокод для commerce-пользователя. Одноразовый код; один юзер может
 * погасить не более одного промо. Идемпотентно: повторное погашение того же кода тем же юзером
 * возвращает тот же redemption_ref (чтобы кредит движка на стороне commerce остался идемпотентным).
 * Возвращает номинал, партнёра и его referral_code (для атрибуции незакреплённого юзера).
 */
export async function redeemPromoCode(database: SalesDatabase, input: {
  code: string;
  commerceUserId: string;
}): Promise<PromoRedemption> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const found = await client.query<{
      id: string; partner_id: string; value_nano: string; status: PromoCodeStatus;
      redeemed_by_commerce_user_id: string | null; redemption_ref: string | null; referral_code: string;
    }>(
      `SELECT pc.id, pc.partner_id, pc.value_nano::text AS value_nano, pc.status,
              pc.redeemed_by_commerce_user_id, pc.redemption_ref, p.referral_code
       FROM promo_codes pc JOIN partners p ON p.id = pc.partner_id
       WHERE upper(pc.code) = upper($1)
       FOR UPDATE OF pc`,
      [input.code],
    );
    const promo = found.rows[0];
    if (!promo) {
      await client.query("ROLLBACK");
      throw new PromoNotFoundError("promo code not found");
    }

    // Идемпотентность: тот же код уже погашен этим же юзером → возвращаем прежний ref.
    if (promo.status === "redeemed" && promo.redeemed_by_commerce_user_id === input.commerceUserId) {
      await client.query("ROLLBACK");
      return {
        valueNano: BigInt(promo.value_nano),
        partnerId: promo.partner_id,
        referralCode: promo.referral_code,
        redemptionRef: promo.redemption_ref!,
        alreadyRedeemed: true,
      };
    }
    if (promo.status !== "active") {
      await client.query("ROLLBACK");
      throw new PromoAlreadyRedeemedError("promo code is not redeemable");
    }
    // Один промо на юзера.
    const prior = await client.query(
      "SELECT 1 FROM promo_codes WHERE redeemed_by_commerce_user_id = $1 LIMIT 1",
      [input.commerceUserId],
    );
    if ((prior.rowCount ?? 0) > 0) {
      await client.query("ROLLBACK");
      throw new UserAlreadyRedeemedError("this account has already used a promo code");
    }
    const ref = `promo:${promo.id}`;
    await client.query(
      `UPDATE promo_codes
       SET status = 'redeemed', redeemed_by_commerce_user_id = $2, redeemed_at = now(), redemption_ref = $3
       WHERE id = $1`,
      [promo.id, input.commerceUserId, ref],
    );
    await client.query(
      `INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
       VALUES ('user', $1, 'promo.redeemed', 'promo_code', $2, $3::jsonb)`,
      [input.commerceUserId, promo.id, JSON.stringify({ partnerId: promo.partner_id, valueNano: promo.value_nano })],
    );
    await client.query("COMMIT");
    return {
      valueNano: BigInt(promo.value_nano),
      partnerId: promo.partner_id,
      referralCode: promo.referral_code,
      redemptionRef: ref,
      alreadyRedeemed: false,
    };
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    client.release();
  }
}

/** Админ: включить/выключить промо и задать лимиты. */
export async function setPromoPermissions(database: SalesDatabase, partnerId: string, input: {
  enabled: boolean;
  maxValueNano: bigint;
  maxCount: number;
  actorId: string;
}): Promise<boolean> {
  const result = await database.pool.query(
    `UPDATE partners
     SET promo_enabled = $2, promo_max_value_nano = $3, promo_max_count = $4, updated_at = now()
     WHERE id = $1`,
    [partnerId, input.enabled, input.maxValueNano.toString(), input.maxCount],
  );
  if ((result.rowCount ?? 0) === 0) return false;
  await database.pool.query(
    `INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
     VALUES ('admin', $1, 'promo.permissions', 'partner', $2, $3::jsonb)`,
    [input.actorId, partnerId, JSON.stringify({ enabled: input.enabled, maxValueNano: input.maxValueNano.toString(), maxCount: input.maxCount })],
  );
  return true;
}

function isUniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && (error as { code?: string }).code === "23505";
}
