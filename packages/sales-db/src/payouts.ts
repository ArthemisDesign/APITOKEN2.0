import type { SalesDatabase } from "./client.js";
import { PARTNER_ACCOUNTING_LOCK_KEY } from "./reversal-accounting.js";

export type PayoutStatus = "requested" | "approved" | "paid" | "rejected";
export type PayoutDecision = "approve" | "reject" | "paid";

export interface Payout {
  id: string;
  partnerId: string;
  amountNano: bigint;
  status: PayoutStatus;
  method: string;
  details: unknown;
  requestedAt: Date;
  decidedAt: Date | null;
  paidAt: Date | null;
  adminNote: string | null;
  txHash: string | null;
  chainStatus: string | null;
}

export class InsufficientEarningsError extends Error {}
export class InvalidPayoutTransitionError extends Error {}

interface PayoutRow {
  id: string;
  partner_id: string;
  amount_nano: string;
  status: PayoutStatus;
  method: string;
  details: unknown;
  requested_at: Date;
  decided_at: Date | null;
  paid_at: Date | null;
  admin_note: string | null;
  tx_hash: string | null;
  chain_status: string | null;
}

const PAYOUT_COLUMNS = `
  id, partner_id, amount_nano::text AS amount_nano, status, method, details,
  requested_at, decided_at, paid_at, admin_note, tx_hash, chain_status
`;

function mapPayout(row: PayoutRow): Payout {
  return {
    id: row.id,
    partnerId: row.partner_id,
    amountNano: BigInt(row.amount_nano),
    status: row.status,
    method: row.method,
    details: row.details,
    requestedAt: row.requested_at,
    decidedAt: row.decided_at,
    paidAt: row.paid_at,
    adminNote: row.admin_note,
    txHash: row.tx_hash,
    chainStatus: row.chain_status,
  };
}

/**
 * Legacy single-row/manual payout writer. The scheduled on-chain path uses createPayoutBatch(),
 * which creates the batch and all requested rows atomically at the locked period boundary.
 * The request writer takes the accounting fence, locks the partner row and subtracts
 * requested/approved/paid commitments,
 * so a manual integration cannot overcommit balance concurrently with batch preparation.
 * The current API has no request caller. Existing legacy rows may only be rejected; all positive
 * decisions use the source-head-fenced on-chain batch flow.
 */
export async function requestPayout(database: SalesDatabase, input: {
  partnerId: string;
  amountNano: bigint;
  method: string;
  details: unknown;
}): Promise<Payout> {
  if (input.amountNano <= 0n) throw new InsufficientEarningsError("payout amount must be positive");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    await client.query("SELECT pg_advisory_xact_lock($1)", [PARTNER_ACCOUNTING_LOCK_KEY]);
    // Serialize concurrent payout requests per partner.
    await client.query("SELECT id FROM partners WHERE id = $1 FOR UPDATE", [input.partnerId]);
    const balance = await client.query<{ available: string }>(`
      SELECT (
        COALESCE((SELECT SUM(amount_nano) FROM (
                    SELECT partner_id, amount_nano FROM commission_entries
                    UNION ALL
                    SELECT partner_id, amount_nano FROM commission_entries_v2
                  ) all_commissions WHERE partner_id = $1), 0)
        + COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments
                    WHERE partner_id = $1), 0)
        - COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE partner_id = $1 AND status IN ('requested', 'approved', 'paid')), 0)
      )::text AS available
    `, [input.partnerId]);
    const availableNano = BigInt(balance.rows[0]?.available ?? "0");
    if (input.amountNano > availableNano) {
      await client.query("ROLLBACK");
      throw new InsufficientEarningsError("payout amount exceeds available earnings");
    }
    const inserted = await client.query<PayoutRow>(`
      INSERT INTO payouts (partner_id, amount_nano, method, details)
      VALUES ($1, $2, $3, $4::jsonb)
      RETURNING ${PAYOUT_COLUMNS}
    `, [input.partnerId, input.amountNano.toString(), input.method, JSON.stringify(input.details ?? {})]);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'payout.requested', 'payout', $2, $3::jsonb)
    `, [input.partnerId, inserted.rows[0]!.id, JSON.stringify({ amountNano: input.amountNano.toString(), method: input.method })]);
    await client.query("COMMIT");
    return mapPayout(inserted.rows[0]!);
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function listPartnerPayouts(database: SalesDatabase, partnerId: string): Promise<Payout[]> {
  const result = await database.pool.query<PayoutRow>(`
    SELECT ${PAYOUT_COLUMNS} FROM payouts WHERE partner_id = $1 ORDER BY requested_at DESC
  `, [partnerId]);
  return result.rows.map(mapPayout);
}

export async function listPayouts(database: SalesDatabase, status?: PayoutStatus): Promise<Payout[]> {
  const result = status
    ? await database.pool.query<PayoutRow>(
        `SELECT ${PAYOUT_COLUMNS} FROM payouts WHERE status = $1 ORDER BY requested_at DESC`,
        [status],
      )
    : await database.pool.query<PayoutRow>(
        `SELECT ${PAYOUT_COLUMNS} FROM payouts ORDER BY requested_at DESC`,
      );
  return result.rows.map(mapPayout);
}

const DECISION_TRANSITIONS: Record<PayoutDecision, { from: PayoutStatus[]; to: PayoutStatus }> = {
  approve: { from: ["requested"], to: "approved" },
  reject: { from: ["requested", "approved"], to: "rejected" },
  paid: { from: ["requested", "approved"], to: "paid" },
};

export async function decidePayout(database: SalesDatabase, input: {
  payoutId: string;
  decision: PayoutDecision;
  note: string | null;
  actorId: string | null;
}): Promise<Payout> {
  const transition = DECISION_TRANSITIONS[input.decision];
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    if (input.decision !== "reject") {
      await client.query("ROLLBACK");
      throw new InvalidPayoutTransitionError(
        "legacy manual payouts can only be rejected; use the fenced on-chain batch flow",
      );
    }
    const current = await client.query<PayoutRow & { batch_id: string | null }>(`
      SELECT ${PAYOUT_COLUMNS}, batch_id
      FROM payouts
      WHERE id = $1
      FOR UPDATE
    `, [input.payoutId]);
    const selected = current.rows[0];
    if (!selected || selected.batch_id !== null || !transition.from.includes(selected.status)) {
      await client.query("ROLLBACK");
      throw new InvalidPayoutTransitionError("payout is not in a state that allows this decision");
    }
    const result = await client.query<PayoutRow>(`
      UPDATE payouts
      SET status = $2::payout_status,
          decided_at = CASE WHEN $2::payout_status IN ('approved', 'rejected') THEN now() ELSE COALESCE(decided_at, now()) END,
          paid_at = CASE WHEN $2::payout_status = 'paid' THEN now() ELSE paid_at END,
          admin_note = COALESCE($3, admin_note)
      WHERE id = $1 AND status = ANY($4::payout_status[])
        -- НЕ трогаем строки, которыми управляет on-chain батч-движок (у них есть batch_id): иначе
        -- ручной reject/paid может «освободить» уже отправленную транзакцию → двойная выплата.
        AND batch_id IS NULL
      RETURNING ${PAYOUT_COLUMNS}
    `, [input.payoutId, transition.to, input.note, transition.from]);
    const row = result.rows[0];
    if (!row) {
      await client.query("ROLLBACK");
      throw new InvalidPayoutTransitionError("payout is not in a state that allows this decision");
    }
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, $2, 'payout', $3, $4::jsonb)
    `, [
      input.actorId, `payout.${input.decision}`, row.id,
      JSON.stringify({ amountNano: row.amount_nano, partnerId: row.partner_id, note: input.note }),
    ]);
    await client.query("COMMIT");
    return mapPayout(row);
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
