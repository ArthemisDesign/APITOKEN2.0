import type { SalesDatabase } from "./client.js";

export type SyncFeed = "attributions" | "usage_events" | "topups";

export interface ReferredUserSummary {
  commerceUserId: string;
  attributedAt: Date;
  spendNano: bigint;
  earnedNano: bigint;
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
  }>(`
    SELECT ru.commerce_user_id, ru.attributed_at,
      COALESCE((
        SELECT SUM(pue.amount_nano) FROM partner_usage_events pue
        WHERE pue.commerce_user_id = ru.commerce_user_id
      ), 0)::text AS spend_nano,
      COALESCE((
        SELECT SUM(ce.amount_nano)
        FROM commission_entries ce
        JOIN partner_usage_events e ON e.id = ce.usage_event_id
        WHERE ce.partner_id = $1 AND e.commerce_user_id = ru.commerce_user_id
      ), 0)::text AS earned_nano
    FROM referred_users ru
    WHERE ru.partner_id = $1
    ORDER BY ru.attributed_at DESC
  `, [partnerId]);
  return result.rows.map((row) => ({
    commerceUserId: row.commerce_user_id,
    attributedAt: row.attributed_at,
    spendNano: BigInt(row.spend_nano),
    earnedNano: BigInt(row.earned_nano),
  }));
}

export async function insertReferredTopup(database: SalesDatabase, input: {
  commercePaymentId: string;
  commerceUserId: string;
  amountNano: bigint;
  paidAt: Date;
}): Promise<boolean> {
  // Insert only when the user is referred; idempotent on commerce_payment_id.
  const result = await database.pool.query(`
    INSERT INTO referred_topups (commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at)
    SELECT $1, ru.commerce_user_id, ru.partner_id, $3, $4
    FROM referred_users ru WHERE ru.commerce_user_id = $2
    ON CONFLICT (commerce_payment_id) DO NOTHING
  `, [input.commercePaymentId, input.commerceUserId, input.amountNano.toString(), input.paidAt]);
  return (result.rowCount ?? 0) > 0;
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
