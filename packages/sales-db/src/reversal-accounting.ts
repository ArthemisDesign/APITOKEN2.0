import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";

/** Shared money fence: every negative adjustment writer and payout prepare/send takes this lock. */
export const PARTNER_ACCOUNTING_LOCK_KEY = 918273647;

/**
 * Holds the partner money fence on one dedicated PostgreSQL session. Payout sending keeps this
 * lease from its final local balance proof through signing and broadcast, while reversal writers
 * take the matching transaction-scoped lock. Callers must release the returned client in finally.
 */
export async function acquirePartnerAccountingLock(database: SalesDatabase): Promise<PoolClient> {
  const client = await database.pool.connect();
  try {
    await client.query("SELECT pg_advisory_lock($1)", [PARTNER_ACCOUNTING_LOCK_KEY]);
    return client;
  } catch (error) {
    client.release();
    throw error;
  }
}

export async function releasePartnerAccountingLock(client: PoolClient): Promise<void> {
  try {
    await client.query("SELECT pg_advisory_unlock($1)", [PARTNER_ACCOUNTING_LOCK_KEY]);
  } finally {
    client.release();
  }
}

export interface PaidFundingLotSource {
  commerceTopupId: bigint;
  commercePaymentId: string;
  commerceUserId: string;
  amountNano: bigint;
  paidAt: Date;
}

export interface PaymentReversalSource {
  commerceReversalId: bigint;
  commercePaymentId: string;
  commerceUserId: string;
  kind: "refund" | "dispute";
  amountNano: bigint;
  reversedAt: Date;
}

export class PartnerFundingReplayConflictError extends Error {
  constructor(kind: "funding lot" | "payment reversal", sourceId: string) {
    super(`${kind} replay conflicts with immutable source ${sourceId}`);
    this.name = "PartnerFundingReplayConflictError";
  }
}

/**
 * Snapshots one commit-ordered topups-v2 row after the ordinary topup consumer has created its
 * referred_topups evidence. Missing evidence is an ordering error and leaves the independent
 * funding-lot cursor behind for retry. Exact replay is a no-op; any changed source field fails loud.
 */
export async function recordPaidFundingLot(
  database: SalesDatabase,
  source: PaidFundingLotSource,
): Promise<"recorded" | "duplicate"> {
  if (source.commerceTopupId <= 0n || source.amountNano <= 0n) {
    throw new Error("paid funding lot source must contain positive identifiers and money");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const topup = await client.query<{
      id: string;
      commerce_user_id: string;
      partner_id: string;
      amount_nano: string;
      paid_at: Date;
    }>(`
      SELECT id::text, commerce_user_id, partner_id,
             amount_nano::text AS amount_nano, paid_at
      FROM referred_topups
      WHERE commerce_payment_id = $1
      FOR SHARE
    `, [source.commercePaymentId]);
    const referred = topup.rows[0];
    if (!referred) {
      throw new Error(`paid funding lot ${source.commerceTopupId} is waiting for its referred topup`);
    }
    if (
      referred.commerce_user_id !== source.commerceUserId
      || referred.amount_nano !== source.amountNano.toString()
      || referred.paid_at.getTime() !== source.paidAt.getTime()
    ) {
      throw new PartnerFundingReplayConflictError("funding lot", source.commerceTopupId.toString());
    }

    const inserted = await client.query<{ id: string }>(`
      INSERT INTO partner_paid_funding_lots(
        referred_topup_id, commerce_topup_id, commerce_payment_id,
        commerce_user_id, partner_id, original_amount_nano, paid_at
      ) VALUES($1, $2, $3, $4, $5, $6, $7)
      ON CONFLICT DO NOTHING
      RETURNING id::text
    `, [
      referred.id, source.commerceTopupId.toString(), source.commercePaymentId,
      source.commerceUserId, referred.partner_id, source.amountNano.toString(), source.paidAt,
    ]);
    if (!inserted.rows[0]) {
      const existing = await client.query<{
        referred_topup_id: string;
        commerce_topup_id: string;
        commerce_payment_id: string;
        commerce_user_id: string;
        partner_id: string;
        original_amount_nano: string;
        paid_at: Date;
      }>(`
        SELECT referred_topup_id::text, commerce_topup_id::text, commerce_payment_id,
               commerce_user_id, partner_id, original_amount_nano::text, paid_at
        FROM partner_paid_funding_lots
        WHERE commerce_payment_id = $1 OR commerce_topup_id = $2 OR referred_topup_id = $3
        FOR SHARE
      `, [source.commercePaymentId, source.commerceTopupId.toString(), referred.id]);
      const stored = existing.rows[0];
      if (
        existing.rows.length !== 1
        || !stored
        || stored.referred_topup_id !== referred.id
        || stored.commerce_topup_id !== source.commerceTopupId.toString()
        || stored.commerce_payment_id !== source.commercePaymentId
        || stored.commerce_user_id !== source.commerceUserId
        || stored.partner_id !== referred.partner_id
        || stored.original_amount_nano !== source.amountNano.toString()
        || stored.paid_at.getTime() !== source.paidAt.getTime()
      ) {
        throw new PartnerFundingReplayConflictError("funding lot", source.commerceTopupId.toString());
      }
      await client.query("COMMIT");
      return "duplicate";
    }
    await client.query("COMMIT");
    return "recorded";
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

type IncompleteUsage = {
  source_schema: 1 | 2;
  usage_id: string;
  commerce_user_id: string;
  partner_id: string;
  basis_nano: string;
  occurred_at: Date;
  commerce_event_id: string;
};

/**
 * Reconciles a bounded prefix of locally stored usage in deterministic event order. Each usage is
 * allocated atomically across causally available payment lots in topup sequence order. A partial
 * allocation is retained when source topups are still catching up; the next usage cannot overtake
 * it because both this writer and the database trigger enforce the same order.
 */
export async function reconcilePartnerFundingEvidence(
  database: SalesDatabase,
  limit = 200,
): Promise<{ examined: number; completed: number }> {
  const candidates = await database.pool.query<IncompleteUsage>(`
    WITH all_usage AS (
      SELECT 1::int AS source_schema, usage.id AS usage_id,
             usage.commerce_user_id, usage.partner_id, usage.amount_nano AS basis_nano,
             usage.occurred_at, usage.commerce_event_id
      FROM partner_usage_events usage
      UNION ALL
      SELECT 2::int AS source_schema, usage.id AS usage_id,
             usage.commerce_user_id, usage.partner_id, usage.paid_funded_nano AS basis_nano,
             usage.occurred_at, usage.commerce_event_id
      FROM partner_usage_events_v2 usage
    )
    SELECT source_schema, usage_id::text, commerce_user_id, partner_id,
           basis_nano::text, occurred_at, commerce_event_id::text
    FROM all_usage usage
    WHERE COALESCE((
      SELECT sum(allocation.allocated_paid_nano)
      FROM partner_usage_funding_allocations allocation
      WHERE (usage.source_schema = 1 AND allocation.usage_event_id = usage.usage_id)
         OR (usage.source_schema = 2 AND allocation.usage_event_v2_id = usage.usage_id)
    ), 0) < usage.basis_nano
    ORDER BY occurred_at, commerce_event_id, source_schema
    LIMIT $1
  `, [limit]);

  let examined = 0;
  let completed = 0;
  const blockedUsers = new Set<string>();
  for (const usage of candidates.rows) {
    if (blockedUsers.has(usage.commerce_user_id)) continue;
    examined += 1;
    if (await allocateOneUsage(database, usage)) completed += 1;
    else blockedUsers.add(usage.commerce_user_id);
  }
  // This also heals a crash from an older/manual allocation commit that completed usage without
  // creating all deterministic commission slices.
  await reconcileCommissionFundingEvidence(database, limit * 12);
  return { examined, completed };
}

async function allocateOneUsage(database: SalesDatabase, usage: IncompleteUsage): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock($1)", [PARTNER_ACCOUNTING_LOCK_KEY]);
    await client.query(`
      SET CONSTRAINTS partner_reversed_usage_complete_guard,
        partner_reversed_commission_complete_guard,
        partner_reversal_adjustment_set_guard DEFERRED
    `);
    // Same per-user mutex as the database allocation guard. It prevents two reconcilers from
    // consuming one lot concurrently and makes the computed remaining amounts stable.
    const owner = await client.query<{ partner_id: string }>(`
      SELECT partner_id
      FROM referred_users
      WHERE commerce_user_id = $1
      FOR UPDATE
    `, [usage.commerce_user_id]);
    if (owner.rows[0]?.partner_id !== usage.partner_id) {
      throw new Error(`usage ${usage.source_schema}:${usage.usage_id} lost its referral owner`);
    }
    const allocated = await client.query<{ amount: string }>(`
      SELECT COALESCE(sum(allocated_paid_nano), 0)::text AS amount
      FROM partner_usage_funding_allocations
      WHERE ($1::int = 1 AND usage_event_id = $2)
         OR ($1::int = 2 AND usage_event_v2_id = $2)
    `, [usage.source_schema, usage.usage_id]);
    let remaining = BigInt(usage.basis_nano) - BigInt(allocated.rows[0]!.amount);
    if (remaining < 0n) throw new Error(`usage ${usage.source_schema}:${usage.usage_id} is over-allocated`);

    const lots = await client.query<{
      id: string;
      remaining_nano: string;
    }>(`
      SELECT lot.id::text,
             (lot.original_amount_nano - COALESCE((
               SELECT sum(allocation.allocated_paid_nano)
               FROM partner_usage_funding_allocations allocation
               WHERE allocation.funding_lot_id = lot.id
             ), 0))::text AS remaining_nano
      FROM partner_paid_funding_lots lot
      WHERE lot.commerce_user_id = $1
        AND lot.partner_id = $2
        AND lot.paid_at <= $3
        AND NOT EXISTS (
          SELECT 1 FROM partner_payment_reversals reversal
          WHERE reversal.funding_lot_id = lot.id AND reversal.reversed_at <= $3
        )
        AND lot.original_amount_nano > COALESCE((
          SELECT sum(allocation.allocated_paid_nano)
          FROM partner_usage_funding_allocations allocation
          WHERE allocation.funding_lot_id = lot.id
        ), 0)
      ORDER BY lot.commerce_topup_id
      FOR UPDATE OF lot
    `, [usage.commerce_user_id, usage.partner_id, usage.occurred_at]);

    for (const lot of lots.rows) {
      if (remaining === 0n) break;
      const available = BigInt(lot.remaining_nano);
      const amount = available < remaining ? available : remaining;
      if (amount <= 0n) continue;
      const inserted = await client.query(`
        INSERT INTO partner_usage_funding_allocations(
          funding_lot_id, usage_event_id, usage_event_v2_id, allocated_paid_nano
        ) VALUES($1, $2, $3, $4)
        ON CONFLICT DO NOTHING
      `, [
        lot.id,
        usage.source_schema === 1 ? usage.usage_id : null,
        usage.source_schema === 2 ? usage.usage_id : null,
        amount.toString(),
      ]);
      if ((inserted.rowCount ?? 0) > 0) remaining -= amount;
    }

    if (remaining === 0n) {
      await insertCommissionSlicesForUsage(client, usage.source_schema, usage.usage_id);
      await insertLateReversalAdjustmentsForUsage(client, usage.source_schema, usage.usage_id);
    }
    await client.query("COMMIT");
    return remaining === 0n;
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

async function insertLateReversalAdjustmentsForUsage(
  client: PoolClient,
  sourceSchema: 1 | 2,
  usageId: string,
): Promise<void> {
  const usageColumn = sourceSchema === 1 ? "usage_event_id" : "usage_event_v2_id";
  await client.query(`
    INSERT INTO partner_commission_adjustments(
      reversal_id, commission_funding_allocation_id, partner_id,
      amount_nano, effective_at
    )
    SELECT reversal.id, commission_allocation.id,
           COALESCE(entry.partner_id, entry_v2.partner_id),
           -commission_allocation.allocated_commission_nano, reversal.reversed_at
    FROM partner_usage_funding_allocations usage_allocation
    JOIN partner_payment_reversals reversal
      ON reversal.funding_lot_id = usage_allocation.funding_lot_id
    JOIN partner_commission_funding_allocations commission_allocation
      ON commission_allocation.usage_funding_allocation_id = usage_allocation.id
    LEFT JOIN commission_entries entry
      ON entry.id = commission_allocation.commission_entry_id
    LEFT JOIN commission_entries_v2 entry_v2
      ON entry_v2.id = commission_allocation.commission_entry_v2_id
    WHERE usage_allocation.${usageColumn} = $1
      AND commission_allocation.allocated_commission_nano > 0
    ON CONFLICT DO NOTHING
  `, [usageId]);
}

async function insertCommissionSlicesForUsage(
  client: PoolClient,
  sourceSchema: 1 | 2,
  usageId: string,
): Promise<void> {
  const usageColumn = sourceSchema === 1 ? "usage_event_id" : "usage_event_v2_id";
  const commissionTable = sourceSchema === 1 ? "commission_entries" : "commission_entries_v2";
  const commissionColumn = sourceSchema === 1 ? "commission_entry_id" : "commission_entry_v2_id";
  const basisColumn = sourceSchema === 1 ? "usage.amount_nano" : "entry.base_paid_funded_nano";
  const usageTable = sourceSchema === 1 ? "partner_usage_events" : "partner_usage_events_v2";
  await client.query(`
    WITH ordered AS (
      SELECT allocation.id AS usage_allocation_id,
             allocation.allocated_paid_nano,
             sum(allocation.allocated_paid_nano) OVER (
               ORDER BY lot.commerce_topup_id ROWS UNBOUNDED PRECEDING
             ) AS cumulative_paid_nano
      FROM partner_usage_funding_allocations allocation
      JOIN partner_paid_funding_lots lot ON lot.id = allocation.funding_lot_id
      WHERE allocation.${usageColumn} = $1
    )
    INSERT INTO partner_commission_funding_allocations(
      usage_funding_allocation_id, ${commissionColumn}, allocated_commission_nano
    )
    SELECT ordered.usage_allocation_id, entry.id,
      (
        floor(ordered.cumulative_paid_nano::numeric * entry.amount_nano::numeric
              / ${basisColumn}::numeric)
        - floor((ordered.cumulative_paid_nano - ordered.allocated_paid_nano)::numeric
                * entry.amount_nano::numeric / ${basisColumn}::numeric)
      )::bigint
    FROM ordered
    JOIN ${commissionTable} entry ON entry.usage_event_id = $1
    JOIN ${usageTable} usage ON usage.id = entry.usage_event_id
    ON CONFLICT DO NOTHING
  `, [usageId]);
}

async function reconcileCommissionFundingEvidence(database: SalesDatabase, limit: number): Promise<void> {
  const missing = await database.pool.query<{ source_schema: 1 | 2; usage_id: string }>(`
    WITH missing AS (
      SELECT 1::int AS source_schema, usage.id AS usage_id
      FROM partner_usage_events usage
      WHERE COALESCE((
        SELECT sum(allocation.allocated_paid_nano)
        FROM partner_usage_funding_allocations allocation
        WHERE allocation.usage_event_id = usage.id
      ), 0) = usage.amount_nano
        AND EXISTS (
          SELECT 1
          FROM partner_usage_funding_allocations usage_allocation
          JOIN commission_entries entry ON entry.usage_event_id = usage.id
          WHERE usage_allocation.usage_event_id = usage.id
            AND NOT EXISTS (
              SELECT 1 FROM partner_commission_funding_allocations commission_allocation
              WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
                AND commission_allocation.commission_entry_id = entry.id
            )
        )
      UNION ALL
      SELECT 2::int AS source_schema, usage.id AS usage_id
      FROM partner_usage_events_v2 usage
      WHERE COALESCE((
        SELECT sum(allocation.allocated_paid_nano)
        FROM partner_usage_funding_allocations allocation
        WHERE allocation.usage_event_v2_id = usage.id
      ), 0) = usage.paid_funded_nano
        AND EXISTS (
          SELECT 1
          FROM partner_usage_funding_allocations usage_allocation
          JOIN commission_entries_v2 entry ON entry.usage_event_id = usage.id
          WHERE usage_allocation.usage_event_v2_id = usage.id
            AND NOT EXISTS (
              SELECT 1 FROM partner_commission_funding_allocations commission_allocation
              WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
                AND commission_allocation.commission_entry_v2_id = entry.id
            )
        )
    )
    SELECT source_schema, usage_id::text FROM missing LIMIT $1
  `, [limit]);
  for (const row of missing.rows) {
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
      await client.query("SELECT pg_advisory_xact_lock($1)", [PARTNER_ACCOUNTING_LOCK_KEY]);
      await client.query(`
        SET CONSTRAINTS partner_reversed_commission_complete_guard,
          partner_reversal_adjustment_set_guard DEFERRED
      `);
      await insertCommissionSlicesForUsage(client, row.source_schema, row.usage_id);
      await insertLateReversalAdjustmentsForUsage(client, row.source_schema, row.usage_id);
      await client.query("COMMIT");
    } catch (error) {
      await client.query("ROLLBACK").catch(() => undefined);
      throw error;
    } finally {
      client.release();
    }
  }
}

/** True only when every locally stored usage and commission row has complete funding evidence. */
export async function hasIncompletePartnerFundingEvidence(database: SalesDatabase): Promise<boolean> {
  const result = await database.pool.query<{ incomplete: boolean }>(`
    SELECT EXISTS (
      SELECT 1 FROM partner_usage_events usage
      WHERE COALESCE((
        SELECT sum(allocation.allocated_paid_nano)
        FROM partner_usage_funding_allocations allocation
        WHERE allocation.usage_event_id = usage.id
      ), 0) <> usage.amount_nano
      UNION ALL
      SELECT 1 FROM partner_usage_events_v2 usage
      WHERE COALESCE((
        SELECT sum(allocation.allocated_paid_nano)
        FROM partner_usage_funding_allocations allocation
        WHERE allocation.usage_event_v2_id = usage.id
      ), 0) <> usage.paid_funded_nano
      UNION ALL
      SELECT 1
      FROM partner_usage_funding_allocations usage_allocation
      JOIN commission_entries entry ON entry.usage_event_id = usage_allocation.usage_event_id
      WHERE NOT EXISTS (
        SELECT 1 FROM partner_commission_funding_allocations commission_allocation
        WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
          AND commission_allocation.commission_entry_id = entry.id
      )
      UNION ALL
      SELECT 1
      FROM partner_usage_funding_allocations usage_allocation
      JOIN commission_entries_v2 entry ON entry.usage_event_id = usage_allocation.usage_event_v2_id
      WHERE NOT EXISTS (
        SELECT 1 FROM partner_commission_funding_allocations commission_allocation
        WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
          AND commission_allocation.commission_entry_v2_id = entry.id
      )
    ) AS incomplete
  `);
  return result.rows[0]?.incomplete ?? true;
}

/**
 * Applies one canonical source page and advances its source watermark in the same SERIALIZABLE
 * transaction. Exact crash replay is harmless; conflicting immutable evidence aborts the page.
 */
export async function recordPaymentReversalPage(
  database: SalesDatabase,
  events: readonly PaymentReversalSource[],
  nextCursor: bigint,
): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock($1)", [PARTNER_ACCOUNTING_LOCK_KEY]);
    await client.query(`
      SET CONSTRAINTS partner_reversal_adjustment_set_guard,
        partner_reversal_insert_complete_guard DEFERRED
    `);
    for (const event of events) await insertPaymentReversal(client, event);
    await client.query(`
      INSERT INTO sync_cursors(feed, last_id) VALUES('payment_reversals', $1)
      ON CONFLICT (feed) DO UPDATE
      SET last_id = GREATEST(sync_cursors.last_id, EXCLUDED.last_id), updated_at = now()
    `, [nextCursor.toString()]);
    await client.query(`
      SET CONSTRAINTS partner_reversal_adjustment_set_guard,
        partner_reversal_insert_complete_guard IMMEDIATE
    `);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

/** Bounded proof used by payout/read rollout and production validation. */
export async function getPartnerReversalAccountingHealth(database: SalesDatabase): Promise<{
  fundingLotCursor: bigint;
  paymentReversalCursor: bigint;
  incompleteUsageCount: bigint;
  missingCommissionSliceCount: bigint;
  reversalCount: bigint;
  adjustmentCount: bigint;
  adjustmentNano: bigint;
  incompleteReversalCount: bigint;
}> {
  const result = await database.pool.query<{
    funding_lot_cursor: string;
    payment_reversal_cursor: string;
    incomplete_usage_count: string;
    missing_commission_slice_count: string;
    reversal_count: string;
    adjustment_count: string;
    adjustment_nano: string;
    incomplete_reversal_count: string;
  }>(`
    WITH all_usage AS (
      SELECT 1::int AS source_schema, usage.id, usage.amount_nano AS basis_nano
      FROM partner_usage_events usage
      UNION ALL
      SELECT 2::int AS source_schema, usage.id, usage.paid_funded_nano AS basis_nano
      FROM partner_usage_events_v2 usage
    ), incomplete_usage AS (
      SELECT count(*) AS amount
      FROM all_usage usage
      WHERE COALESCE((
        SELECT sum(allocation.allocated_paid_nano)
        FROM partner_usage_funding_allocations allocation
        WHERE (usage.source_schema = 1 AND allocation.usage_event_id = usage.id)
           OR (usage.source_schema = 2 AND allocation.usage_event_v2_id = usage.id)
      ), 0) <> usage.basis_nano
    ), missing_commission AS (
      SELECT count(*) AS amount
      FROM (
        SELECT 1
        FROM partner_usage_funding_allocations usage_allocation
        JOIN commission_entries entry ON entry.usage_event_id = usage_allocation.usage_event_id
        WHERE NOT EXISTS (
          SELECT 1 FROM partner_commission_funding_allocations commission_allocation
          WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
            AND commission_allocation.commission_entry_id = entry.id
        )
        UNION ALL
        SELECT 1
        FROM partner_usage_funding_allocations usage_allocation
        JOIN commission_entries_v2 entry ON entry.usage_event_id = usage_allocation.usage_event_v2_id
        WHERE NOT EXISTS (
          SELECT 1 FROM partner_commission_funding_allocations commission_allocation
          WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
            AND commission_allocation.commission_entry_v2_id = entry.id
        )
      ) rows
    )
    SELECT
      COALESCE((SELECT last_id FROM sync_cursors WHERE feed='topup_funding_lots'), 0)::text
        AS funding_lot_cursor,
      COALESCE((SELECT last_id FROM sync_cursors WHERE feed='payment_reversals'), 0)::text
        AS payment_reversal_cursor,
      incomplete_usage.amount::text AS incomplete_usage_count,
      missing_commission.amount::text AS missing_commission_slice_count,
      (SELECT count(*)::text FROM partner_payment_reversals) AS reversal_count,
      (SELECT count(*)::text FROM partner_commission_adjustments) AS adjustment_count,
      (SELECT COALESCE(sum(amount_nano), 0)::text FROM partner_commission_adjustments)
        AS adjustment_nano,
      (SELECT count(*)::text
       FROM partner_payment_reversals reversal
       WHERE EXISTS (
         SELECT 1
         FROM partner_commission_funding_allocations commission_allocation
         JOIN partner_usage_funding_allocations usage_allocation
           ON usage_allocation.id = commission_allocation.usage_funding_allocation_id
         WHERE usage_allocation.funding_lot_id = reversal.funding_lot_id
           AND commission_allocation.allocated_commission_nano > 0
           AND NOT EXISTS (
             SELECT 1 FROM partner_commission_adjustments adjustment
             WHERE adjustment.reversal_id = reversal.id
               AND adjustment.commission_funding_allocation_id = commission_allocation.id
               AND adjustment.amount_nano = -commission_allocation.allocated_commission_nano
           )
       )) AS incomplete_reversal_count
    FROM incomplete_usage, missing_commission
  `);
  const row = result.rows[0]!;
  return {
    fundingLotCursor: BigInt(row.funding_lot_cursor),
    paymentReversalCursor: BigInt(row.payment_reversal_cursor),
    incompleteUsageCount: BigInt(row.incomplete_usage_count),
    missingCommissionSliceCount: BigInt(row.missing_commission_slice_count),
    reversalCount: BigInt(row.reversal_count),
    adjustmentCount: BigInt(row.adjustment_count),
    adjustmentNano: BigInt(row.adjustment_nano),
    incompleteReversalCount: BigInt(row.incomplete_reversal_count),
  };
}

export interface PartnerPayoutAccountingProof
  extends Awaited<ReturnType<typeof getPartnerReversalAccountingHealth>> {
  usageCursor: bigint;
  expectedUsageHead: bigint;
  expectedFundingLotHead: bigint;
  expectedPaymentReversalHead: bigint;
  ready: boolean;
  reasons: string[];
}

/**
 * Exact local proof against source heads freshly observed by SyncService. This deliberately does
 * not infer a remote head from time or row counts: payout callers must first drain the HTTP feeds.
 */
export async function getPartnerPayoutAccountingProof(
  database: SalesDatabase,
  expected: {
    usageEvents: bigint;
    fundingLots: bigint;
    paymentReversals: bigint;
  },
): Promise<PartnerPayoutAccountingProof> {
  const [health, usageCursor] = await Promise.all([
    getPartnerReversalAccountingHealth(database),
    database.pool.query<{ last_id: string }>(`
      SELECT COALESCE((SELECT last_id FROM sync_cursors WHERE feed='usage_events'), 0)::text AS last_id
    `).then((result) => BigInt(result.rows[0]!.last_id)),
  ]);
  const reasons: string[] = [];
  if (usageCursor !== expected.usageEvents) reasons.push("usage cursor is behind its source head");
  if (health.fundingLotCursor !== expected.fundingLots) {
    reasons.push("funding-lot cursor is behind its source head");
  }
  if (health.paymentReversalCursor !== expected.paymentReversals) {
    reasons.push("payment-reversal cursor is behind its source head");
  }
  if (health.incompleteUsageCount !== 0n) reasons.push("usage funding allocation is incomplete");
  if (health.missingCommissionSliceCount !== 0n) reasons.push("commission funding slices are incomplete");
  if (health.incompleteReversalCount !== 0n) reasons.push("a payment reversal is not fully reflected");
  return {
    ...health,
    usageCursor,
    expectedUsageHead: expected.usageEvents,
    expectedFundingLotHead: expected.fundingLots,
    expectedPaymentReversalHead: expected.paymentReversals,
    ready: reasons.length === 0,
    reasons,
  };
}

async function insertPaymentReversal(client: PoolClient, event: PaymentReversalSource): Promise<void> {
  if (event.commerceReversalId <= 0n || event.amountNano <= 0n) {
    throw new Error("payment reversal source must contain positive identifiers and money");
  }
  const lot = await client.query<{
    id: string;
    commerce_user_id: string;
    original_amount_nano: string;
  }>(`
    SELECT id::text, commerce_user_id, original_amount_nano::text
    FROM partner_paid_funding_lots
    WHERE commerce_payment_id = $1
    FOR SHARE
  `, [event.commercePaymentId]);
  const fundingLot = lot.rows[0];
  if (!fundingLot) {
    throw new Error(`payment reversal ${event.commerceReversalId} is waiting for its funding lot`);
  }
  if (
    fundingLot.commerce_user_id !== event.commerceUserId
    || fundingLot.original_amount_nano !== event.amountNano.toString()
  ) {
    throw new PartnerFundingReplayConflictError("payment reversal", event.commerceReversalId.toString());
  }

  const inserted = await client.query<{ id: string }>(`
    INSERT INTO partner_payment_reversals(
      commerce_reversal_id, funding_lot_id, commerce_payment_id,
      commerce_user_id, kind, original_amount_nano, reversed_at
    ) VALUES($1, $2, $3, $4, $5, $6, $7)
    ON CONFLICT DO NOTHING
    RETURNING id::text
  `, [
    event.commerceReversalId.toString(), fundingLot.id, event.commercePaymentId,
    event.commerceUserId, event.kind, event.amountNano.toString(), event.reversedAt,
  ]);
  let reversalId = inserted.rows[0]?.id;
  if (!reversalId) {
    const existing = await client.query<{
      id: string;
      commerce_reversal_id: string;
      funding_lot_id: string;
      commerce_payment_id: string;
      commerce_user_id: string;
      kind: string;
      original_amount_nano: string;
      reversed_at: Date;
    }>(`
      SELECT id::text, commerce_reversal_id::text, funding_lot_id::text,
             commerce_payment_id, commerce_user_id, kind,
             original_amount_nano::text, reversed_at
      FROM partner_payment_reversals
      WHERE commerce_reversal_id = $1 OR funding_lot_id = $2 OR commerce_payment_id = $3
      FOR SHARE
    `, [event.commerceReversalId.toString(), fundingLot.id, event.commercePaymentId]);
    const stored = existing.rows[0];
    if (
      existing.rows.length !== 1
      || !stored
      || stored.commerce_reversal_id !== event.commerceReversalId.toString()
      || stored.funding_lot_id !== fundingLot.id
      || stored.commerce_payment_id !== event.commercePaymentId
      || stored.commerce_user_id !== event.commerceUserId
      || stored.kind !== event.kind
      || stored.original_amount_nano !== event.amountNano.toString()
      || stored.reversed_at.getTime() !== event.reversedAt.getTime()
    ) {
      throw new PartnerFundingReplayConflictError("payment reversal", event.commerceReversalId.toString());
    }
    reversalId = stored.id;
  }

  await client.query(`
    INSERT INTO partner_commission_adjustments(
      reversal_id, commission_funding_allocation_id, partner_id,
      amount_nano, effective_at
    )
    SELECT $1, commission_allocation.id,
           COALESCE(entry.partner_id, entry_v2.partner_id),
           -commission_allocation.allocated_commission_nano, $2
    FROM partner_commission_funding_allocations commission_allocation
    JOIN partner_usage_funding_allocations usage_allocation
      ON usage_allocation.id = commission_allocation.usage_funding_allocation_id
    LEFT JOIN commission_entries entry
      ON entry.id = commission_allocation.commission_entry_id
    LEFT JOIN commission_entries_v2 entry_v2
      ON entry_v2.id = commission_allocation.commission_entry_v2_id
    WHERE usage_allocation.funding_lot_id = $3
      AND commission_allocation.allocated_commission_nano > 0
    ON CONFLICT DO NOTHING
  `, [reversalId, event.reversedAt, fundingLot.id]);
}
