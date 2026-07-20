import type { SalesDatabase } from "./client.js";

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
}

const PAYOUT_COLUMNS = `
  id, partner_id, amount_nano::text AS amount_nano, status, method, details,
  requested_at, decided_at, paid_at, admin_note
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
  };
}

/**
 * ⚠️ ЗНАЙ ПЕРЕД ПОДКЛЮЧЕНИЕМ ОПЛАТ: это ЕДИНСТВЕННАЯ функция, создающая строки в `payouts`, и она
 * СЕЙЧАС НИ ОТКУДА НЕ ВЫЗЫВАЕТСЯ (нет ни админ-маршрута, ни крона). Следствия, пока не подключено:
 *   • `getDuePayoutList`/`getPartnerPeriodState`/`getPartnerEarningsTotals` вычитают committed-выплаты,
 *     но таблица пуста → вычитается 0, поэтому due-list — это ПРЕВЬЮ, а не журнал;
 *   • факт on-chain оплаты нигде не фиксируется → тот же партнёр будет показан «к оплате» в каждом окне.
 * Когда будем подключать реальные выплаты — завести админ-маршрут «сгенерировать выплаты из окна»
 * (пишет `requested` строки за окно) ИЛИ чтобы внешний исполнитель писал `paid` строки. До этого
 * выплаты ведём вручную вне системы. См. аудит 2026-07-20 (B1).
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
    await client.query("BEGIN");
    // Serialize concurrent payout requests per partner.
    await client.query("SELECT id FROM partners WHERE id = $1 FOR UPDATE", [input.partnerId]);
    const balance = await client.query<{ available: string }>(`
      SELECT (
        COALESCE((SELECT SUM(amount_nano) FROM commission_entries WHERE partner_id = $1), 0)
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
    await client.query("BEGIN");
    const result = await client.query<PayoutRow>(`
      UPDATE payouts
      SET status = $2::payout_status,
          decided_at = CASE WHEN $2::payout_status IN ('approved', 'rejected') THEN now() ELSE COALESCE(decided_at, now()) END,
          paid_at = CASE WHEN $2::payout_status = 'paid' THEN now() ELSE paid_at END,
          admin_note = COALESCE($3, admin_note)
      WHERE id = $1 AND status = ANY($4::payout_status[])
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
