import type { SalesDatabase } from "./client.js";

export type SyncFeed = "attributions" | "usage_events" | "topups";

export interface ReferredUserSummary {
  commerceUserId: string;
  attributedAt: Date;
  spendNano: bigint;
  earnedNano: bigint;
  topupNano: bigint;
}

export async function upsertReferredUser(database: SalesDatabase, input: {
  commerceUserId: string;
  partnerId: string;
  referralCode: string;
  attributedAt: Date;
  sourceAttributionId: bigint;
}): Promise<boolean> {
  const result = await database.pool.query(`
    INSERT INTO referred_users (commerce_user_id, partner_id, referral_code, attributed_at, source_attribution_id)
    VALUES ($1, $2, $3, $4, $5)
    ON CONFLICT (commerce_user_id) DO NOTHING
  `, [input.commerceUserId, input.partnerId, input.referralCode, input.attributedAt, input.sourceAttributionId.toString()]);
  return (result.rowCount ?? 0) > 0;
}

/** Партнёр, за которым закреплён пользователь (или null). Нужно для идемпотентной атрибуции: после
 * краша между upsert и побочными эффектами повтор увидит won=false, но владельца можно перечитать. */
export async function getReferredUserPartner(database: SalesDatabase, commerceUserId: string): Promise<string | null> {
  const r = await database.pool.query<{ partner_id: string }>(
    "SELECT partner_id FROM referred_users WHERE commerce_user_id = $1",
    [commerceUserId],
  );
  return r.rows[0]?.partner_id ?? null;
}

/**
 * Разрешает маскированную ссылку на реферала (первые 8 hex uuid — ровно то, что видит партнёр как
 * `user-XXXXXXXX…`) в полный commerce_user_id, СТРОГО в границах рефералов одного партнёра.
 * Полный uuid наружу не выходит: и партнёрка, и админка оперируют только префиксом.
 * null — не найден; "ambiguous" — коллизия префикса (теоретическая), пусть вызывающий отдаст 409.
 */
export async function resolveReferredUserByPrefix(
  database: SalesDatabase,
  partnerId: string,
  prefix: string,
): Promise<string | null | "ambiguous"> {
  if (!/^[0-9a-f]{8}$/.test(prefix)) return null;
  const result = await database.pool.query<{ commerce_user_id: string }>(
    "SELECT commerce_user_id FROM referred_users WHERE partner_id = $1 AND commerce_user_id::text LIKE $2 LIMIT 2",
    [partnerId, `${prefix}%`],
  );
  if (result.rows.length === 0) return null;
  if (result.rows.length > 1) return "ambiguous";
  return result.rows[0]!.commerce_user_id;
}

export async function countReferredUsers(database: SalesDatabase, partnerId: string): Promise<number> {
  const result = await database.pool.query<{ count: string }>(
    "SELECT count(*)::text AS count FROM referred_users WHERE partner_id = $1",
    [partnerId],
  );
  return Number(result.rows[0]?.count ?? "0");
}

export async function listReferredUsers(database: SalesDatabase, partnerId: string): Promise<ReferredUserSummary[]> {
  const result = await database.pool.query<{
    commerce_user_id: string;
    attributed_at: Date;
    spend_nano: string;
    earned_nano: string;
    topup_nano: string;
  }>(`
    SELECT ru.commerce_user_id, ru.attributed_at,
      COALESCE((
        SELECT SUM(amount_nano) FROM (
          SELECT commerce_user_id, amount_nano FROM partner_usage_events
          UNION ALL
          SELECT commerce_user_id, paid_funded_nano FROM partner_usage_events_v2
        ) pue
        WHERE pue.commerce_user_id = ru.commerce_user_id
      ), 0)::text AS spend_nano,
      COALESCE((
        SELECT SUM(amount_nano) FROM (
          SELECT ce.partner_id, ce.amount_nano, e.commerce_user_id
          FROM commission_entries ce
          JOIN partner_usage_events e ON e.id = ce.usage_event_id
          UNION ALL
          SELECT ce.partner_id, ce.amount_nano, e.commerce_user_id
          FROM commission_entries_v2 ce
          JOIN partner_usage_events_v2 e ON e.id = ce.usage_event_id
        ) earned
        WHERE earned.partner_id = $1 AND earned.commerce_user_id = ru.commerce_user_id
      ), 0)::text AS earned_nano,
      COALESCE((
        SELECT SUM(rt.amount_nano) FROM referred_topups rt
        WHERE rt.commerce_user_id = ru.commerce_user_id
      ), 0)::text AS topup_nano
    FROM referred_users ru
    WHERE ru.partner_id = $1
    ORDER BY ru.attributed_at DESC
  `, [partnerId]);
  return result.rows.map((row) => ({
    commerceUserId: row.commerce_user_id,
    attributedAt: row.attributed_at,
    spendNano: BigInt(row.spend_nano),
    earnedNano: BigInt(row.earned_nano),
    topupNano: BigInt(row.topup_nano),
  }));
}

export async function getSyncCursor(database: SalesDatabase, feed: SyncFeed): Promise<bigint> {
  const result = await database.pool.query<{ last_id: string }>(
    "SELECT last_id::text AS last_id FROM sync_cursors WHERE feed = $1",
    [feed],
  );
  return BigInt(result.rows[0]?.last_id ?? "0");
}

export async function advanceSyncCursor(database: SalesDatabase, feed: SyncFeed, lastId: bigint): Promise<void> {
  await database.pool.query(`
    INSERT INTO sync_cursors (feed, last_id) VALUES ($1, $2)
    ON CONFLICT (feed) DO UPDATE
    SET last_id = GREATEST(sync_cursors.last_id, EXCLUDED.last_id), updated_at = now()
  `, [feed, lastId.toString()]);
}
