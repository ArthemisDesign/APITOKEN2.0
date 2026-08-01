import { and, asc, eq, gt, lt, sql } from "drizzle-orm";
import type { Database } from "./client.js";
import {
  payments,
  pricingUsageAttributions,
  pricingUsageEvents,
  referralAttributions,
} from "./schema.js";

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
  providerId: string | null;
  accountClass: "b2c" | null;
  pricingMode: "track" | null;
  paidFundedNano: bigint | null;
  commissionEligible: true | null;
  snapshotDigest: string | null;
  occurredAt: Date;
}

export interface TopupFeedRow {
  id: bigint;
  paymentId: string;
  userId: string;
  amountNano: bigint;
  paidAt: Date;
}

export interface SalesFeedPage<T> {
  items: T[];
  // The source watermark advances even when every row in the scanned page belongs to an ordinary
  // customer. Without it, sales would repeatedly scan the same filtered tail forever.
  nextCursor: bigint;
}

/** Идемпотентно записывает атрибуцию регистрации к реф-коду (первый код побеждает). */
export async function recordReferralAttribution(database: Database, userId: string, code: string): Promise<void> {
  await database.db
    .insert(referralAttributions)
    .values({ userId, code })
    .onConflictDoNothing({ target: referralAttributions.userId });
}

/** Реф-код, по которому пользователь пришёл (или null). Нужен для синхронного применения скидки-«пола»
 * сразу при провижининге движок-аккаунта — чтобы реферал видел свою ставку, не дожидаясь sales-фида. */
export async function getReferralAttributionCode(database: Database, userId: string): Promise<string | null> {
  const rows = await database.db
    .select({ code: referralAttributions.code })
    .from(referralAttributions)
    .where(eq(referralAttributions.userId, userId))
    .limit(1);
  return rows[0]?.code ?? null;
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
): Promise<SalesFeedPage<UsageEventFeedRow>> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  // Limit applies to the source stream before filtering. Every source row therefore advances the
  // watermark, including unreferred, static-price, service, and zero-paid rows; otherwise an
  // ineligible tail would be rescanned forever.
  const rows = await database.db
    .select({
      id: pricingUsageEvents.feedSeq,
      userId: pricingUsageEvents.userId,
      legacyRealFundedNano: pricingUsageEvents.realFundedNano,
      occurredAt: pricingUsageEvents.occurredAt,
      attributedUserId: referralAttributions.userId,
      attributionEventId: pricingUsageAttributions.pricingUsageEventId,
      attributionSchemaVersion: pricingUsageAttributions.attributionSchemaVersion,
      snapshotKind: pricingUsageAttributions.snapshotKind,
      providerId: pricingUsageAttributions.providerId,
      accountClass: pricingUsageAttributions.accountClass,
      pricingMode: pricingUsageAttributions.pricingMode,
      paidFundedNano: pricingUsageAttributions.paidFundedNano,
      commissionEligible: pricingUsageAttributions.commissionEligible,
      snapshotDigest: pricingUsageAttributions.snapshotDigest,
    })
    .from(pricingUsageEvents)
    // The sales database cannot distinguish a temporarily late attribution from a customer who
    // was never referred. Filter at the commerce authority, where that distinction is durable, so
    // ordinary customer spend cannot accumulate forever in pending_referral_events.
    .leftJoin(referralAttributions, eq(referralAttributions.userId, pricingUsageEvents.userId))
    .leftJoin(
      pricingUsageAttributions,
      eq(pricingUsageAttributions.pricingUsageEventId, pricingUsageEvents.id),
    )
    .where(and(gt(pricingUsageEvents.feedSeq, afterId), lt(pricingUsageEvents.createdAt, lagCutoff)))
    .orderBy(asc(pricingUsageEvents.feedSeq))
    .limit(limit);
  return {
    items: rows.flatMap((row): UsageEventFeedRow[] => {
      if (row.attributedUserId === null) return [];

      // Compatibility window for ledger rows ingested before immutable attribution was delivered.
      // Their only available commission basis is the old free-first projection.
      if (row.attributionEventId === null) {
        if (row.legacyRealFundedNano <= 0n) return [];
        return [{
          id: row.id,
          userId: row.userId,
          amountNano: row.legacyRealFundedNano,
          providerId: null,
          accountClass: null,
          pricingMode: null,
          paidFundedNano: null,
          commissionEligible: null,
          snapshotDigest: null,
          occurredAt: row.occurredAt,
        }];
      }

      // New commission authority is deliberately narrower than "has paid funding". Only an exact
      // supported policy snapshot in the B2C track can cross the sales boundary. Static discounts,
      // B2B/service traffic, non-commissionable rules, unknown schema versions, and zero-paid rows
      // are durable source rows but never commission events.
      if (
        row.attributionSchemaVersion !== 1n
        || row.snapshotKind !== "policy_v1"
        || row.providerId === null
        || row.providerId.length === 0
        || row.accountClass !== "b2c"
        || row.pricingMode !== "track"
        || row.paidFundedNano === null
        || row.paidFundedNano <= 0n
        || row.commissionEligible !== true
        || row.snapshotDigest === null
        || row.snapshotDigest.length === 0
      ) return [];

      return [{
        id: row.id,
        userId: row.userId,
        amountNano: row.paidFundedNano,
        providerId: row.providerId,
        accountClass: "b2c",
        pricingMode: "track",
        paidFundedNano: row.paidFundedNano,
        commissionEligible: true,
        snapshotDigest: row.snapshotDigest,
        occurredAt: row.occurredAt,
      }];
    }),
    nextCursor: rows.at(-1)?.id ?? afterId,
  };
}

/**
 * Оплаченные пополнения. Курсор — микросекунды epoch от paid_at (НЕ feed_seq: paid_at
 * проставляется позже insert, и просроченный feed_seq выпал бы из курсора навсегда).
 */
export async function listPaidTopupsAfter(
  database: Database,
  afterId: bigint,
  limit: number,
): Promise<SalesFeedPage<TopupFeedRow>> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  const paidMicros = sql<string>`(extract(epoch from ${payments.paidAt}) * 1000000)::bigint`;
  const rows = await database.db
    .select({
      id: paidMicros,
      paymentId: payments.id,
      userId: payments.userId,
      amountNano: payments.amountNano,
      paidAt: payments.paidAt,
      attributedUserId: referralAttributions.userId,
    })
    .from(payments)
    .leftJoin(referralAttributions, eq(referralAttributions.userId, payments.userId))
    .where(and(
      eq(payments.status, "paid"),
      gt(paidMicros, sql`${afterId}`),
      lt(payments.paidAt, lagCutoff),
    ))
    .orderBy(asc(paidMicros))
    .limit(limit);
  return {
    items: rows
      .filter((row) => row.attributedUserId !== null)
      .map((row) => ({
        id: BigInt(row.id),
        paymentId: row.paymentId,
        userId: row.userId,
        amountNano: row.amountNano,
        paidAt: row.paidAt as Date,
      })),
    nextCursor: rows.at(-1) ? BigInt(rows.at(-1)!.id) : afterId,
  };
}

// Профиль реферала для витрины партнёра: тип (b2b/b2c), скидка/floor и маппинг на engine-аккаунт
// (баланс читается уже из движка вызывающей стороной). Только по явному списку user_id — партнёр
// видит исключительно закреплённых за ним пользователей (sales-api ограничивает список).
export interface ReferralProfileRow {
  userId: string;
  customerType: "b2c" | "b2b";
  multiplierBp: number;
  referralFloorBps: number;
  cumulativeTopupNano: bigint;
  engineAccountId: string | null;
  engineStatus: string | null;
}

export async function listReferralProfiles(database: Database, userIds: readonly string[]): Promise<ReferralProfileRow[]> {
  if (userIds.length === 0) return [];
  // engine_accounts уникален по user_id → LEFT JOIN не даёт дублей.
  const result = await database.pool.query<{
    user_id: string;
    customer_type: "b2c" | "b2b";
    multiplier_bp: number;
    referral_floor_bps: number;
    cumulative_topup_nano: string;
    engine_account_id: string | null;
    engine_status: string | null;
  }>(`
    SELECT cp.user_id, cp.customer_type, cp.multiplier_bp, cp.referral_floor_bps,
           cp.cumulative_topup_nano, ea.engine_account_id, ea.status AS engine_status
    FROM customer_profiles cp
    LEFT JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE cp.user_id = ANY($1::uuid[])
  `, [userIds as string[]]);
  return result.rows.map((row) => ({
    userId: row.user_id,
    customerType: row.customer_type,
    multiplierBp: row.multiplier_bp,
    referralFloorBps: row.referral_floor_bps,
    cumulativeTopupNano: BigInt(row.cumulative_topup_nano),
    engineAccountId: row.engine_account_id,
    engineStatus: row.engine_status,
  }));
}
