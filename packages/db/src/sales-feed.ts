import { and, asc, eq, gt, lt, sql } from "drizzle-orm";
import type { Database } from "./client.js";
import { payments, pricingUsageEvents, referralAttributions } from "./schema.js";

// Internal-фид для sales bounded context (sales.apitoken.sale). Читатель хранит курсор
// last_id и запрашивает `after_id` — как ledger-фид Control API движка. Строки моложе
// FEED_VISIBILITY_LAG_MS скрываются: bigserial присваивается на insert, и уже закоммиченная
// строка с большим seq может стать видимой раньше in-flight строки с меньшим — лаг закрывает окно.
const FEED_VISIBILITY_LAG_MS = 10_000;

export interface ReferralAttributionFeedRow {
  id: bigint;
  userId: string;
  code: string;
  createdAt: Date;
}

export interface UsageEventFeedRow {
  id: bigint;
  userId: string;
  amountNano: bigint;
  occurredAt: Date;
}

export interface TopupFeedRow {
  id: bigint;
  paymentId: string;
  userId: string;
  amountNano: bigint;
  paidAt: Date;
}

/** Идемпотентно записывает атрибуцию регистрации к реф-коду (первый код побеждает). */
export async function recordReferralAttribution(database: Database, userId: string, code: string): Promise<void> {
  await database.db
    .insert(referralAttributions)
    .values({ userId, code })
    .onConflictDoNothing({ target: referralAttributions.userId });
}

export async function listReferralAttributionsAfter(
  database: Database,
  afterId: bigint,
  limit: number,
): Promise<ReferralAttributionFeedRow[]> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  return database.db
    .select({
      id: referralAttributions.id,
      userId: referralAttributions.userId,
      code: referralAttributions.code,
      createdAt: referralAttributions.createdAt,
    })
    .from(referralAttributions)
    .where(and(gt(referralAttributions.id, afterId), lt(referralAttributions.createdAt, lagCutoff)))
    .orderBy(asc(referralAttributions.id))
    .limit(limit);
}

export async function listUsageEventsAfter(
  database: Database,
  afterId: bigint,
  limit: number,
): Promise<UsageEventFeedRow[]> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  return database.db
    .select({
      id: pricingUsageEvents.feedSeq,
      userId: pricingUsageEvents.userId,
      amountNano: pricingUsageEvents.amountNano,
      occurredAt: pricingUsageEvents.occurredAt,
    })
    .from(pricingUsageEvents)
    .where(and(gt(pricingUsageEvents.feedSeq, afterId), lt(pricingUsageEvents.createdAt, lagCutoff)))
    .orderBy(asc(pricingUsageEvents.feedSeq))
    .limit(limit);
}

/**
 * Оплаченные пополнения. Курсор — микросекунды epoch от paid_at (НЕ feed_seq: paid_at
 * проставляется позже insert, и просроченный feed_seq выпал бы из курсора навсегда).
 */
export async function listPaidTopupsAfter(database: Database, afterId: bigint, limit: number): Promise<TopupFeedRow[]> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  const paidMicros = sql<string>`(extract(epoch from ${payments.paidAt}) * 1000000)::bigint`;
  const rows = await database.db
    .select({
      id: paidMicros,
      paymentId: payments.id,
      userId: payments.userId,
      amountNano: payments.amountNano,
      paidAt: payments.paidAt,
    })
    .from(payments)
    .where(and(
      eq(payments.status, "paid"),
      gt(paidMicros, sql`${afterId}`),
      lt(payments.paidAt, lagCutoff),
    ))
    .orderBy(asc(paidMicros))
    .limit(limit);
  return rows.map((row) => ({
    id: BigInt(row.id),
    paymentId: row.paymentId,
    userId: row.userId,
    amountNano: row.amountNano,
    paidAt: row.paidAt as Date,
  }));
}
