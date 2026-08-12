import { and, asc, eq, gt, gte, lt, sql } from "drizzle-orm";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import {
  payments,
  pricingUsageEvents,
  referralAttributions,
} from "./schema.js";

// Internal-фид для sales bounded context (sales.apitoken.sale). Читатель хранит курсор
// last_id и запрашивает `after_id` — как ledger-фид Control API движка. Строки моложе
// FEED_VISIBILITY_LAG_MS скрываются. Writers additionally serialize sequence allocation through
// a table lock: the lag remains a rolling-deploy safeguard for the previous binary, while the lock
// makes commit order match cursor order once the new writer is active.
const FEED_VISIBILITY_LAG_MS = 10_000;

export class AmbiguousTopupCursorBoundaryError extends Error {
  constructor(cursor: bigint) {
    super(`paid top-up cursor boundary is ambiguous at ${cursor}`);
    this.name = "AmbiguousTopupCursorBoundaryError";
  }
}

export class ReferralAttributionConflictError extends Error {
  constructor(readonly userId: string) {
    super(`user ${userId} is already attributed to another partner`);
    this.name = "ReferralAttributionConflictError";
  }
}

export interface ReferralAttributionFeedRow {
  id: bigint;
  userId: string;
  code: string;
  createdAt: Date;
}

export interface UsageEventFeedRow {
  id: bigint;
  userId: string;
  /**
   * The commission basis: the part of the charge the customer paid with their own money, under
   * free-first accounting over the amount the engine actually collected. Free credit and the
   * pool-funded settlement shortfall never become commission. The retired per-request policy
   * attribution carried a second, parallel basis; there is only this one now, and the fields it
   * populated stay in the payload as nulls so the sales contract does not shrink.
   */
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

export interface TopupV2FeedRow extends TopupFeedRow {
  /** Commit-ordered paid-row insertion sequence; independent of provider timestamps. */
  id: bigint;
}

export interface PaymentReversalFeedRow {
  /** Commit-ordered audit-log sequence allocated by the terminal payment transaction. */
  id: bigint;
  paymentId: string;
  userId: string;
  kind: "refund" | "dispute";
  amountNano: bigint;
  reversedAt: Date;
}

export interface SalesFeedPage<T> {
  items: T[];
  // The source watermark advances even when every row in the scanned page belongs to an ordinary
  // customer. Without it, sales would repeatedly scan the same filtered tail forever.
  nextCursor: bigint;
}

/**
 * Transactional attribution writer for signup/OAuth. Keeping this inside the account-creation
 * transaction means a successful registration can never exist without the referral row that its
 * request carried.
 */
export async function recordReferralAttributionTx(
  client: PoolClient,
  userId: string,
  code: string,
): Promise<void> {
  // bigserial is allocated before COMMIT. Serializing all inserts prevents a later id from
  // becoming visible first and moving the sales cursor past an older in-flight attribution.
  // SHARE ROW EXCLUSIVE also fences the previous binary's ordinary ROW EXCLUSIVE insert during
  // a rolling deployment, so the protection does not depend on every process changing at once.
  await client.query("LOCK TABLE referral_attributions IN SHARE ROW EXCLUSIVE MODE");
  await client.query(`
    INSERT INTO referral_attributions (user_id, code)
    VALUES ($1, $2)
    ON CONFLICT (user_id) DO NOTHING
  `, [userId, code]);
  const stored = await client.query<{ code: string }>(`
    SELECT code FROM referral_attributions WHERE user_id = $1
  `, [userId]);
  if (stored.rows[0]?.code !== code) {
    throw new ReferralAttributionConflictError(userId);
  }
}

/** Идемпотентно записывает атрибуцию к реф-коду отдельной атомарной транзакцией. */
export async function recordReferralAttribution(database: Database, userId: string, code: string): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await recordReferralAttributionTx(client, userId, code);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Referral code recorded at signup (or null). The activation path uses it to atomically consume
 * legacy one-time attribution links; the async Sales feed retries that marker replay. */
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
  // watermark, including unreferred and zero-paid rows; otherwise an ineligible tail would be
  // rescanned forever.
  const rows = await database.db
    .select({
      id: pricingUsageEvents.feedSeq,
      userId: pricingUsageEvents.userId,
      realFundedNano: pricingUsageEvents.realFundedNano,
      providerId: pricingUsageEvents.providerId,
      occurredAt: pricingUsageEvents.occurredAt,
      attributedUserId: referralAttributions.userId,
    })
    .from(pricingUsageEvents)
    // The sales database cannot distinguish a temporarily late row from a customer who was never
    // referred. Filter at the commerce authority, where that distinction is durable, so ordinary
    // customer spend cannot accumulate forever in pending_referral_events.
    .leftJoin(referralAttributions, and(
      eq(referralAttributions.userId, pricingUsageEvents.userId),
      // A referral acquired later must never capture spend that happened before attribution.
      gte(pricingUsageEvents.occurredAt, referralAttributions.createdAt),
    ))
    .where(and(gt(pricingUsageEvents.feedSeq, afterId), lt(pricingUsageEvents.createdAt, lagCutoff)))
    .orderBy(asc(pricingUsageEvents.feedSeq))
    .limit(limit);
  return {
    items: rows.flatMap((row): UsageEventFeedRow[] => {
      // Only referred spend funded by the customer's own money crosses the sales boundary.
      if (row.attributedUserId === null || row.realFundedNano <= 0n) return [];
      return [{
        id: row.id,
        userId: row.userId,
        amountNano: row.realFundedNano,
        providerId: row.providerId,
        accountClass: null,
        pricingMode: null,
        paidFundedNano: null,
        commissionEligible: null,
        snapshotDigest: null,
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
    .leftJoin(referralAttributions, and(
      eq(referralAttributions.userId, payments.userId),
      // Top-up history is partner-visible only from the durable attribution instant onward.
      gte(payments.paidAt, referralAttributions.createdAt),
    ))
    .where(and(
      eq(payments.status, "paid"),
      gt(paidMicros, sql`${afterId}`),
      lt(payments.paidAt, lagCutoff),
    ))
    .orderBy(asc(paidMicros), asc(payments.id))
    // One look-ahead row proves that the timestamp-only cursor does not split an equal-paid_at
    // group. A split cannot be resumed safely without a composite cursor, so fail closed instead
    // of silently skipping the remainder of the group.
    .limit(limit + 1);
  const pageRows = rows.slice(0, limit);
  const lookahead = rows[limit];
  const last = pageRows.at(-1);
  if (last && lookahead && BigInt(last.id) === BigInt(lookahead.id)) {
    throw new AmbiguousTopupCursorBoundaryError(BigInt(last.id));
  }
  return {
    items: pageRows
      .filter((row) => row.attributedUserId !== null)
      .map((row) => ({
        id: BigInt(row.id),
        paymentId: row.paymentId,
        userId: row.userId,
        amountNano: row.amountNano,
        paidAt: row.paidAt as Date,
      })),
    nextCursor: last ? BigInt(last.id) : afterId,
  };
}

/**
 * Additive successor to the timestamp-cursor feed. The source page is ordered and limited by the
 * commit-ordered payments.feed_seq before referral filtering, so every source row advances the
 * watermark and equal provider paid_at timestamps remain independently resumable. A payments row
 * is created only from a verified paid event; a later refund changes its status but not the fact
 * that this deposit occurred, so status is intentionally not a replay filter.
 */
export async function listPaidTopupsV2After(
  database: Database,
  afterId: bigint,
  limit: number,
): Promise<SalesFeedPage<TopupV2FeedRow>> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  const rows = await database.db
    .select({
      id: payments.feedSeq,
      paymentId: payments.id,
      userId: payments.userId,
      amountNano: payments.amountNano,
      paidAt: payments.paidAt,
      attributedUserId: referralAttributions.userId,
    })
    .from(payments)
    .leftJoin(referralAttributions, and(
      eq(referralAttributions.userId, payments.userId),
      gte(payments.paidAt, referralAttributions.createdAt),
    ))
    .where(and(gt(payments.feedSeq, afterId), lt(payments.createdAt, lagCutoff)))
    .orderBy(asc(payments.feedSeq))
    .limit(limit);

  const items = rows.flatMap((row): TopupV2FeedRow[] => {
    if (row.paidAt === null) {
      throw new Error(`verified payment ${row.paymentId} has no paid_at`);
    }
    if (row.attributedUserId === null) return [];
    return [{
      id: row.id,
      paymentId: row.paymentId,
      userId: row.userId,
      amountNano: row.amountNano,
      paidAt: row.paidAt,
    }];
  });
  return { items, nextCursor: rows.at(-1)?.id ?? afterId };
}

/**
 * Terminal payment reversals. The immutable audit row is inserted in the same transaction that
 * changes payments.status, so Sales can never observe one side without the other. Limit applies
 * before referral filtering and nextCursor is the whole source-page watermark: an ordinary
 * customer's reversal cannot pin the partner consumer forever. Audit ids are shared with other
 * actions, but selecting the next reversal id remains safe because the cursor is scoped to this
 * feed and only skips non-reversal rows that can never become reversal rows later.
 */
export async function listPaymentReversalsAfter(
  database: Database,
  afterId: bigint,
  limit: number,
): Promise<SalesFeedPage<PaymentReversalFeedRow>> {
  const lagCutoff = new Date(Date.now() - FEED_VISIBILITY_LAG_MS);
  const rows = await database.pool.query<{
    id: string;
    payment_id: string | null;
    user_id: string | null;
    kind: "refund" | "dispute";
    amount_nano: string;
    reversed_at: Date;
    attributed_user_id: string | null;
  }>(`
    WITH source_page AS (
      SELECT audit.id, audit.target_id, audit.metadata, audit.created_at
      FROM audit_log audit
      WHERE audit.action = 'payment.reversed'
        AND audit.id > $1
        AND audit.created_at < $2
      ORDER BY audit.id
      LIMIT $3
    )
    SELECT source.id::text, payment.id AS payment_id, payment.user_id,
           source.metadata->>'kind' AS kind,
           source.metadata->>'amountNano' AS amount_nano,
           source.created_at AS reversed_at,
           attribution.user_id AS attributed_user_id
    FROM source_page source
    LEFT JOIN payments payment ON source.target_id = payment.id::text
    LEFT JOIN referral_attributions attribution
      ON attribution.user_id = payment.user_id
     AND payment.paid_at >= attribution.created_at
    ORDER BY source.id
  `, [afterId.toString(), lagCutoff, limit]);

  const items = rows.rows.flatMap((row): PaymentReversalFeedRow[] => {
    if (row.payment_id === null || row.user_id === null) {
      throw new Error(`payment reversal ${row.id} has no payment`);
    }
    if (row.attributed_user_id === null) return [];
    if (row.kind !== "refund" && row.kind !== "dispute") {
      throw new Error(`payment reversal ${row.id} has an invalid kind`);
    }
    const amountNano = BigInt(row.amount_nano);
    if (amountNano <= 0n) throw new Error(`payment reversal ${row.id} has a non-positive amount`);
    return [{
      id: BigInt(row.id),
      paymentId: row.payment_id,
      userId: row.user_id,
      kind: row.kind,
      amountNano,
      reversedAt: row.reversed_at,
    }];
  });
  return { items, nextCursor: rows.rows.at(-1) === undefined ? afterId : BigInt(rows.rows.at(-1)!.id) };
}

// Referral profile for Sales: type, actual scalar discount, legacy marker and engine mapping.
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
