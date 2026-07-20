import type { SalesDatabase } from "./client.js";

// Пакетные on-chain выплаты. Двухфазно: prepare (собрать кандидатов, посчитать, создать батч в
// статусе 'prepared') → send (по одному, симуляция → бродкаст → подтверждение). Деньги — nanoUSD.

export type PayoutBatchStatus = "preparing" | "prepared" | "sending" | "sent" | "failed" | "canceled";
export type PayoutChainStatus = "pending" | "simulated" | "broadcast" | "confirmed" | "failed";

export interface PayoutCandidate {
  partnerId: string;
  telegramUsername: string | null;
  email: string | null;
  displayName: string | null;
  walletAddress: string;
  unpaidNano: bigint;
}

/**
 * Кандидаты на выплату: АКТИВНЫЕ партнёры с привязанным BEP-20 кошельком и непогашенным балансом
 * (earned − committed[requested/approved/paid]) > minNano. committed включает уже созданные батчем
 * 'requested'-строки, поэтому один и тот же баланс не попадёт в два батча. Адрес берём из
 * payout_details.address (формат уже проверен при привязке; checksum перепроверяет сервис через ethers).
 */
export async function getPayoutCandidates(database: SalesDatabase, minNano: bigint, earnedBefore?: Date): Promise<PayoutCandidate[]> {
  // Учитываем только комиссии, начисленные ДО earnedBefore (конец периода, чьё окно выплат открыто) —
  // так батч уважает 7-дневный лок и совпадает с превью getDuePayoutList. Без границы (undefined) —
  // весь непогашенный остаток. committed вычитаем целиком (по всем статусам, не по времени).
  const result = await database.pool.query<{
    partner_id: string; telegram_username: string | null; email: string | null; display_name: string | null;
    wallet: string | null; unpaid: string;
  }>(`
    SELECT p.id AS partner_id, p.telegram_username, p.email, p.display_name,
      (p.payout_details->>'address') AS wallet,
      (
        COALESCE((SELECT SUM(ce.amount_nano) FROM commission_entries ce WHERE ce.partner_id = p.id AND ($1::timestamptz IS NULL OR ce.created_at < $1)), 0)
        - COALESCE((SELECT SUM(po.amount_nano) FROM payouts po WHERE po.partner_id = p.id AND po.status IN ('requested','approved','paid')), 0)
      ) AS unpaid
    FROM partners p
    WHERE p.status = 'active' AND p.payout_method = 'usdt-bep20' AND (p.payout_details->>'address') IS NOT NULL
  `, [earnedBefore ?? null]);
  return result.rows
    .map((r) => ({
      partnerId: r.partner_id,
      telegramUsername: r.telegram_username,
      email: r.email,
      displayName: r.display_name,
      walletAddress: r.wallet ?? "",
      unpaidNano: BigInt(r.unpaid),
    }))
    .filter((c) => c.walletAddress && c.unpaidNano > minNano);
}

export interface PayoutBatch {
  id: string;
  status: PayoutBatchStatus;
  hotWalletAddress: string | null;
  totalNano: bigint;
  recipientCount: number;
  gasPriceGwei: string | null;
  minNano: bigint;
  note: string | null;
  createdBy: string | null;
  error: string | null;
  createdAt: Date;
  preparedAt: Date | null;
  sentAt: Date | null;
  completedAt: Date | null;
}

export interface PayoutRow {
  id: string;
  partnerId: string;
  telegramUsername: string | null;
  email: string | null;
  displayName: string | null;
  amountNano: bigint;
  status: string;
  walletAddress: string | null;
  txHash: string | null;
  nonce: number | null;
  rawTx: string | null;
  chainStatus: PayoutChainStatus | null;
  chainError: string | null;
  paidAt: Date | null;
}

function mapBatch(r: Record<string, unknown>): PayoutBatch {
  return {
    id: r.id as string,
    status: r.status as PayoutBatchStatus,
    hotWalletAddress: (r.hot_wallet_address as string) ?? null,
    totalNano: BigInt((r.total_nano as string) ?? "0"),
    recipientCount: Number(r.recipient_count),
    gasPriceGwei: (r.gas_price_gwei as string) ?? null,
    minNano: BigInt((r.min_nano as string) ?? "0"),
    note: (r.note as string) ?? null,
    createdBy: (r.created_by as string) ?? null,
    error: (r.error as string) ?? null,
    createdAt: r.created_at as Date,
    preparedAt: (r.prepared_at as Date) ?? null,
    sentAt: (r.sent_at as Date) ?? null,
    completedAt: (r.completed_at as Date) ?? null,
  };
}

/** Есть ли уже незавершённый батч (нельзя готовить второй параллельно). */
export async function getActiveBatch(database: SalesDatabase): Promise<PayoutBatch | null> {
  const r = await database.pool.query(
    "SELECT * FROM payout_batches WHERE status IN ('preparing','prepared','sending') ORDER BY created_at DESC LIMIT 1",
  );
  return r.rows[0] ? mapBatch(r.rows[0]) : null;
}

export class PayoutBatchInProgressError extends Error {}

/**
 * Создаёт батч 'prepared' + payout-строки ('requested', chain_status 'pending') из уже проверенных
 * сервисом кандидатов (адреса — checksummed). Одна транзакция; блокирует создание второго батча,
 * если незавершённый уже есть.
 */
export async function createPayoutBatch(database: SalesDatabase, input: {
  createdBy: string;
  minNano: bigint;
  gasPriceGwei: string;
  hotWalletAddress: string;
  recipients: { partnerId: string; amountNano: bigint; walletAddress: string }[];
}): Promise<PayoutBatch> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // Транзакционный advisory-lock сериализует СОЗДАНИЕ батчей: `SELECT ... FOR UPDATE` ниже на пустом
    // результате ничего не блокирует, поэтому два параллельных prepare() без этого лока могли бы создать
    // два батча и заплатить каждому дважды. Лок снимается при COMMIT/ROLLBACK.
    await client.query("SELECT pg_advisory_xact_lock($1)", [PAYOUT_PREPARE_LOCK_KEY]);
    const existing = await client.query(
      "SELECT 1 FROM payout_batches WHERE status IN ('preparing','prepared','sending')",
    );
    if (existing.rows[0]) {
      await client.query("ROLLBACK");
      throw new PayoutBatchInProgressError("a payout batch is already in progress");
    }
    const total = input.recipients.reduce((s, r) => s + r.amountNano, 0n);
    const batch = await client.query(`
      INSERT INTO payout_batches (status, hot_wallet_address, total_nano, recipient_count, gas_price_gwei, min_nano, created_by, prepared_at)
      VALUES ('prepared', $1, $2, $3, $4, $5, $6, now())
      RETURNING *
    `, [input.hotWalletAddress, total.toString(), input.recipients.length, input.gasPriceGwei, input.minNano.toString(), input.createdBy]);
    const batchId = batch.rows[0]!.id as string;
    for (const r of input.recipients) {
      await client.query(`
        INSERT INTO payouts (partner_id, amount_nano, status, method, details, batch_id, wallet_address, chain_status)
        VALUES ($1, $2, 'requested', 'usdt-bep20', $3::jsonb, $4, $5, 'pending')
      `, [r.partnerId, r.amountNano.toString(), JSON.stringify({ network: "BSC", asset: "USDT (BEP-20)", address: r.walletAddress }), batchId, r.walletAddress]);
    }
    await client.query("COMMIT");
    return mapBatch(batch.rows[0]!);
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function getPayoutBatch(database: SalesDatabase, id: string): Promise<PayoutBatch | null> {
  const r = await database.pool.query("SELECT * FROM payout_batches WHERE id = $1", [id]);
  return r.rows[0] ? mapBatch(r.rows[0]) : null;
}

export async function listPayoutBatches(database: SalesDatabase, limit = 30): Promise<PayoutBatch[]> {
  const r = await database.pool.query("SELECT * FROM payout_batches ORDER BY created_at DESC LIMIT $1", [limit]);
  return r.rows.map(mapBatch);
}

export async function listBatchPayouts(database: SalesDatabase, batchId: string): Promise<PayoutRow[]> {
  const r = await database.pool.query<Record<string, unknown>>(`
    SELECT po.id, po.partner_id, po.amount_nano::text AS amount_nano, po.status, po.wallet_address,
           po.tx_hash, po.nonce, po.raw_tx, po.chain_status, po.chain_error, po.paid_at,
           p.telegram_username, p.email, p.display_name
    FROM payouts po JOIN partners p ON p.id = po.partner_id
    WHERE po.batch_id = $1
    ORDER BY po.amount_nano DESC
  `, [batchId]);
  return r.rows.map((row) => ({
    id: row.id as string,
    partnerId: row.partner_id as string,
    telegramUsername: (row.telegram_username as string) ?? null,
    email: (row.email as string) ?? null,
    displayName: (row.display_name as string) ?? null,
    amountNano: BigInt((row.amount_nano as string) ?? "0"),
    status: row.status as string,
    walletAddress: (row.wallet_address as string) ?? null,
    txHash: (row.tx_hash as string) ?? null,
    nonce: row.nonce === null || row.nonce === undefined ? null : Number(row.nonce),
    rawTx: (row.raw_tx as string) ?? null,
    chainStatus: (row.chain_status as PayoutChainStatus) ?? null,
    chainError: (row.chain_error as string) ?? null,
    paidAt: (row.paid_at as Date) ?? null,
  }));
}

/** Строки, ожидающие отправки: не paid, НЕ rejected (освобождённые повторно не шлём), и без активного
 * бродкаста/подтверждения. Исключение rejected критично: иначе release→re-send = двойная выплата. */
export async function listSendablePayouts(database: SalesDatabase, batchId: string): Promise<PayoutRow[]> {
  return (await listBatchPayouts(database, batchId)).filter(
    (p) => p.status !== "paid" && p.status !== "rejected" && p.chainStatus !== "broadcast" && p.chainStatus !== "confirmed",
  );
}

/** Разблокирует батчи, зависшие в 'sending' без единой 'broadcast'-строки (напр. краш во время
 * отправки): переводит в терминальный 'sent' (completed только если все строки выплачены). */
export async function finalizeStuckSendingBatches(database: SalesDatabase): Promise<number> {
  const r = await database.pool.query(`
    UPDATE payout_batches b
    SET status = 'sent',
        sent_at = COALESCE(sent_at, now()),
        completed_at = CASE WHEN NOT EXISTS (SELECT 1 FROM payouts p WHERE p.batch_id = b.id AND p.status <> 'paid') THEN now() ELSE completed_at END
    WHERE b.status = 'sending'
      AND NOT EXISTS (SELECT 1 FROM payouts p WHERE p.batch_id = b.id AND p.chain_status = 'broadcast')
  `);
  return r.rowCount ?? 0;
}

export async function setBatchStatus(database: SalesDatabase, id: string, status: PayoutBatchStatus, fields: {
  error?: string | null; sent?: boolean; completed?: boolean;
} = {}): Promise<void> {
  await database.pool.query(`
    UPDATE payout_batches
    SET status = $2,
        error = COALESCE($3, error),
        sent_at = CASE WHEN $4 THEN COALESCE(sent_at, now()) ELSE sent_at END,
        completed_at = CASE WHEN $5 THEN now() ELSE completed_at END
    WHERE id = $1
  `, [id, status, fields.error ?? null, fields.sent ?? false, fields.completed ?? false]);
}

// --- on-chain state transitions per payout row (idempotent) ---

export async function markPayoutBroadcast(database: SalesDatabase, payoutId: string, txHash: string, nonce: number, rawTx: string): Promise<boolean> {
  // Защита-в-глубину: не «бродкастим» уже освобождённую (rejected) или выплаченную (paid) строку.
  const r = await database.pool.query(
    "UPDATE payouts SET tx_hash = $2, nonce = $3, raw_tx = $4, chain_status = 'broadcast', chain_error = NULL WHERE id = $1 AND status = 'requested'",
    [payoutId, txHash, nonce, rawTx],
  );
  return (r.rowCount ?? 0) > 0;
}

// Безопасно вернуть баланс в оборот: помечает НЕ отправленную (sim-fail/reverted) строку как
// 'rejected' — sumCommitted её больше не учитывает, деньги попадут в следующее окно. НИКОГДА не
// применять к строке с chain_status='broadcast' (транзакция могла уйти в сеть).
export async function releasePayoutRow(database: SalesDatabase, payoutId: string): Promise<boolean> {
  const r = await database.pool.query(
    "UPDATE payouts SET status = 'rejected', decided_at = now() WHERE id = $1 AND status <> 'paid' AND (chain_status IS NULL OR chain_status IN ('pending','simulated','failed'))",
    [payoutId],
  );
  return (r.rowCount ?? 0) > 0;
}

// Межпроцессный лок отправки (HA): один отправитель на горячий кошелёк. Держится на выделенном
// соединении всю отправку. Ключ фиксированный.
const PAYOUT_SEND_LOCK_KEY = 918273645;
const PAYOUT_PREPARE_LOCK_KEY = 918273646;

/** Наибольший nonce среди ещё не завершённых (broadcast) транзакций — авторитетная «бронь» nonce,
 * чтобы не выдать nonce ≤ уже отправленному (публичный RPC может не видеть in-flight tx BlockRazor). */
export async function getMaxOutstandingNonce(database: SalesDatabase): Promise<number | null> {
  const r = await database.pool.query<{ n: string | null }>(
    "SELECT MAX(nonce)::text AS n FROM payouts WHERE chain_status = 'broadcast' AND nonce IS NOT NULL",
  );
  const n = r.rows[0]?.n;
  return n === null || n === undefined ? null : Number(n);
}

export async function acquirePayoutSendLock(database: SalesDatabase): Promise<import("pg").PoolClient | null> {
  const client = await database.pool.connect();
  try {
    const r = await client.query<{ ok: boolean }>("SELECT pg_try_advisory_lock($1) AS ok", [PAYOUT_SEND_LOCK_KEY]);
    if (!r.rows[0]?.ok) { client.release(); return null; }
    return client;
  } catch (error) {
    client.release();
    throw error;
  }
}

export async function releasePayoutSendLock(client: import("pg").PoolClient): Promise<void> {
  try { await client.query("SELECT pg_advisory_unlock($1)", [PAYOUT_SEND_LOCK_KEY]); }
  finally { client.release(); }
}

/** Помечает выплату подтверждённой. Claim-once: переход учитывается только у той стороны, что реально
 * перевела строку из 'broadcast' → 'confirmed' (returns true), чтобы send() и поллер не слали дубль-
 * уведомление на одну строку. Идемпотентно по деньгам (COALESCE). */
export async function markPayoutConfirmed(database: SalesDatabase, payoutId: string): Promise<boolean> {
  const r = await database.pool.query(
    "UPDATE payouts SET status = 'paid', chain_status = 'confirmed', paid_at = COALESCE(paid_at, now()), decided_at = COALESCE(decided_at, now()) WHERE id = $1 AND chain_status = 'broadcast'",
    [payoutId],
  );
  return (r.rowCount ?? 0) > 0;
}

export async function markPayoutFailed(database: SalesDatabase, payoutId: string, error: string): Promise<void> {
  // Реджект/ревёрт: помечаем failed и СНИМАЕМ tx_hash, чтобы строку можно было отправить заново.
  await database.pool.query(
    "UPDATE payouts SET chain_status = 'failed', chain_error = $2, tx_hash = NULL WHERE id = $1",
    [payoutId, error.slice(0, 500)],
  );
}

/** Строки, отправленные в сеть, но ещё не подтверждённые (для фонового поллера). */
export async function listBroadcastPayouts(database: SalesDatabase, limit = 100): Promise<Array<PayoutRow & { batchId: string | null }>> {
  const r = await database.pool.query<Record<string, unknown>>(`
    SELECT po.id, po.partner_id, po.amount_nano::text AS amount_nano, po.status, po.wallet_address,
           po.tx_hash, po.nonce, po.raw_tx, po.chain_status, po.chain_error, po.paid_at, po.batch_id,
           p.telegram_username, p.email, p.display_name
    FROM payouts po JOIN partners p ON p.id = po.partner_id
    WHERE po.chain_status = 'broadcast' AND po.tx_hash IS NOT NULL
    ORDER BY po.requested_at
    LIMIT $1
  `, [limit]);
  return r.rows.map((row) => ({
    id: row.id as string,
    partnerId: row.partner_id as string,
    telegramUsername: (row.telegram_username as string) ?? null,
    email: (row.email as string) ?? null,
    displayName: (row.display_name as string) ?? null,
    amountNano: BigInt((row.amount_nano as string) ?? "0"),
    status: row.status as string,
    walletAddress: (row.wallet_address as string) ?? null,
    txHash: (row.tx_hash as string) ?? null,
    nonce: row.nonce === null || row.nonce === undefined ? null : Number(row.nonce),
    rawTx: (row.raw_tx as string) ?? null,
    chainStatus: (row.chain_status as PayoutChainStatus) ?? null,
    chainError: (row.chain_error as string) ?? null,
    paidAt: (row.paid_at as Date) ?? null,
    batchId: (row.batch_id as string) ?? null,
  }));
}

/** Отмена подготовленного (ещё не отправленного) батча: снимает строки, возвращает баланс в оборот. */
export async function cancelPayoutBatch(database: SalesDatabase, id: string): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const b = await client.query("SELECT status FROM payout_batches WHERE id = $1 FOR UPDATE", [id]);
    const status = b.rows[0]?.status as string | undefined;
    if (status !== "prepared") { await client.query("ROLLBACK"); return false; }
    // Удаляем только НЕотправленные строки (без подтверждённой выплаты).
    await client.query("DELETE FROM payouts WHERE batch_id = $1 AND status <> 'paid' AND (chain_status IS NULL OR chain_status IN ('pending','failed','simulated'))", [id]);
    await client.query("UPDATE payout_batches SET status = 'canceled', completed_at = now() WHERE id = $1", [id]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
