import type { SalesDatabase } from "./client.js";

// Персональные одноразовые ссылки со скидкой (Part B). Обычный реф-код партнёра = без спец-скидки
// (обычные b2c-тиры); персональная ссылка = отдельный код со скидкой, гаснет первым же привязанным.

export interface DiscountLink {
  id: string;
  code: string;
  discountBps: number;
  note: string | null;
  consumedByCommerceUserId: string | null;
  consumedAt: Date | null;
  createdAt: Date;
}

export class DiscountLinkNotAllowedError extends Error {}
export class DiscountLinkCollisionError extends Error {}

interface DiscountLinkRow {
  id: string;
  code: string;
  discount_bps: number;
  note: string | null;
  consumed_by_commerce_user_id: string | null;
  consumed_at: Date | null;
  created_at: Date;
}

function mapLink(row: DiscountLinkRow): DiscountLink {
  return {
    id: row.id,
    code: row.code,
    discountBps: row.discount_bps,
    note: row.note,
    consumedByCommerceUserId: row.consumed_by_commerce_user_id,
    consumedAt: row.consumed_at,
    createdAt: row.created_at,
  };
}

const LINK_COLUMNS = "id, code, discount_bps, note, consumed_by_commerce_user_id, consumed_at, created_at";

/**
 * Партнёр выпускает персональную ссылку со скидкой. Требует право (`referral_discount_enabled`) и
 * скидку не выше собственного максимума (`referral_discount_bps`). Проверка под блокировкой партнёра.
 */
export async function createDiscountLink(database: SalesDatabase, input: {
  partnerId: string;
  code: string;
  discountBps: number;
  note?: string | null;
}): Promise<DiscountLink> {
  if (input.discountBps <= 0 || input.discountBps > 9500) {
    throw new DiscountLinkNotAllowedError("discount must be between 1 and 95%");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const partner = await client.query<{ referral_discount_enabled: boolean; referral_discount_bps: number; status: string }>(
      `SELECT referral_discount_enabled, referral_discount_bps, status FROM partners WHERE id = $1 FOR UPDATE`,
      [input.partnerId],
    );
    const row = partner.rows[0];
    if (!row || row.status !== "active" || !row.referral_discount_enabled) {
      await client.query("ROLLBACK");
      throw new DiscountLinkNotAllowedError("referral discount is not enabled for this partner");
    }
    if (input.discountBps > row.referral_discount_bps) {
      await client.query("ROLLBACK");
      throw new DiscountLinkNotAllowedError("discount exceeds the partner's allowed maximum");
    }
    let inserted;
    try {
      inserted = await client.query<DiscountLinkRow>(
        `INSERT INTO partner_discount_links (partner_id, code, discount_bps, note)
         VALUES ($1, $2, $3, $4)
         RETURNING ${LINK_COLUMNS}`,
        [input.partnerId, input.code, input.discountBps, input.note?.trim() || null],
      );
    } catch (error) {
      await client.query("ROLLBACK");
      if (isUniqueViolation(error)) throw new DiscountLinkCollisionError("discount link code collision");
      throw error;
    }
    await client.query(
      `INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
       VALUES ('partner', $1, 'discount_link.created', 'discount_link', $2, $3::jsonb)`,
      [input.partnerId, inserted.rows[0]!.id, JSON.stringify({ code: input.code, discountBps: input.discountBps })],
    );
    await client.query("COMMIT");
    return mapLink(inserted.rows[0]!);
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    client.release();
  }
}

export async function listDiscountLinks(database: SalesDatabase, partnerId: string): Promise<DiscountLink[]> {
  const result = await database.pool.query<DiscountLinkRow>(
    `SELECT ${LINK_COLUMNS} FROM partner_discount_links WHERE partner_id = $1 ORDER BY created_at DESC`,
    [partnerId],
  );
  return result.rows.map(mapLink);
}

/** Удаляет персональную ссылку партнёра (только свою). Возвращает true, если удалено. */
export async function deleteDiscountLink(database: SalesDatabase, partnerId: string, linkId: string): Promise<boolean> {
  const result = await database.pool.query(
    "DELETE FROM partner_discount_links WHERE id = $1 AND partner_id = $2",
    [linkId, partnerId],
  );
  return (result.rowCount ?? 0) > 0;
}

export interface ResolvedReferralCode {
  partnerId: string;
  // Скидка, которую даёт код: 0 — обычный реф-код (без скидки), >0 — персональная ссылка.
  discountBps: number;
  // id персональной ссылки, если код именно ссылка (для гашения). null для обычного реф-кода.
  discountLinkId: string | null;
  discountLinkConsumed: boolean;
}

/**
 * Резолвит ?ref= код: обычный реф-код партнёра (discount 0) ИЛИ персональную ссылку (discount>0).
 * Реф-коды 8 симв., ссылки 12 симв. — не пересекаются. null — код никому не принадлежит.
 */
export async function resolveReferralCode(database: SalesDatabase, code: string): Promise<ResolvedReferralCode | null> {
  const partner = await database.pool.query<{ id: string }>(
    "SELECT id FROM partners WHERE referral_code = $1 AND status = 'active'",
    [code],
  );
  if (partner.rows[0]) {
    return { partnerId: partner.rows[0].id, discountBps: 0, discountLinkId: null, discountLinkConsumed: false };
  }
  const link = await database.pool.query<{ id: string; partner_id: string; discount_bps: number; consumed_by_commerce_user_id: string | null }>(
    `SELECT dl.id, dl.partner_id, dl.discount_bps, dl.consumed_by_commerce_user_id
     FROM partner_discount_links dl JOIN partners p ON p.id = dl.partner_id
     WHERE dl.code = $1 AND p.status = 'active'`,
    [code],
  );
  const row = link.rows[0];
  if (!row) return null;
  const consumed = row.consumed_by_commerce_user_id !== null;
  return {
    partnerId: row.partner_id,
    // Гашёная ссылка больше скидку не даёт (одноразовая) — но всё равно закрепляет за партнёром.
    discountBps: consumed ? 0 : row.discount_bps,
    discountLinkId: row.id,
    discountLinkConsumed: consumed,
  };
}

/**
 * АТОМАРНО закрепляет одноразовую ссылку за пользователем (first-wins) и возвращает её скидку,
 * ЕСЛИ пользователь — владелец: только что выиграл гонку ИЛИ уже был владельцем (идемпотентный
 * повтор). Если ссылка уже погашена ДРУГИМ — возвращает 0 (floor не даётся). Обычный реф-код или
 * несуществующий код → 0. Один UPDATE закрывает гонку: одноразовая ссылка НИКОГДА не даёт скидку
 * более чем одному пользователю. Используется и синхронным путём при регистрации, и async-фидом
 * (idempotent backfill владельцу, если синхронное применение упало). Заменяет пару resolve+consume
 * для скидочной части, устраняя окно, где read-only резолв выдавал floor нескольким регистрациям.
 */
export async function claimReferralDiscount(database: SalesDatabase, code: string, commerceUserId: string): Promise<{ discountBps: number }> {
  const claimed = await database.pool.query<{ discount_bps: number }>(
    `UPDATE partner_discount_links dl
     SET consumed_by_commerce_user_id = $2, consumed_at = COALESCE(dl.consumed_at, now())
     FROM partners p
     WHERE dl.code = $1 AND p.id = dl.partner_id AND p.status = 'active'
       AND (dl.consumed_by_commerce_user_id IS NULL OR dl.consumed_by_commerce_user_id = $2)
     RETURNING dl.discount_bps`,
    [code, commerceUserId],
  );
  return { discountBps: claimed.rows[0]?.discount_bps ?? 0 };
}

/** Гасит персональную ссылку первым привязанным пользователем (идемпотентно, first-wins). */
export async function consumeDiscountLink(database: SalesDatabase, linkId: string, commerceUserId: string): Promise<void> {
  await database.pool.query(
    `UPDATE partner_discount_links
     SET consumed_by_commerce_user_id = $2, consumed_at = now()
     WHERE id = $1 AND consumed_by_commerce_user_id IS NULL`,
    [linkId, commerceUserId],
  );
}

function isUniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && (error as { code?: string }).code === "23505";
}
