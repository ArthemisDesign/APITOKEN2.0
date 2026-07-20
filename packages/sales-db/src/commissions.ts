import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";

export const MAX_COMMISSION_LEVELS = 10;

export interface CommissionChainPartner {
  partnerId: string;
  status: PartnerStatus;
  commissionBps: number;
  subCommissionBps: number;
}

export interface CommissionEntryPlan {
  partnerId: string;
  level: number;
  appliedBps: number;
  amountNano: bigint;
}

/**
 * Pure multi-level commission math.
 *
 * `partnersChain[0]` is the direct referrer, `partnersChain[1]` its parent, and so on.
 * Level 0 earns `amountNano * commissionBps / 10000` (integer floor); every next level earns
 * `previousLevelAmount * subCommissionBps / 10000`. The walk stops at the first suspended
 * partner (no entry for it, chain ends there), when a computed amount reaches 0, or beyond
 * level MAX_COMMISSION_LEVELS.
 */
export function computeCommissionChain(
  partnersChain: readonly CommissionChainPartner[],
  amountNano: bigint,
): CommissionEntryPlan[] {
  const entries: CommissionEntryPlan[] = [];
  if (amountNano <= 0n) return entries;
  let basisNano = amountNano;
  for (let level = 0; level < partnersChain.length && level <= MAX_COMMISSION_LEVELS; level += 1) {
    const partner = partnersChain[level]!;
    if (partner.status === "suspended") break;
    const appliedBps = level === 0 ? partner.commissionBps : partner.subCommissionBps;
    const entryAmount = (basisNano * BigInt(appliedBps)) / 10_000n;
    if (entryAmount <= 0n) break;
    entries.push({ partnerId: partner.partnerId, level, appliedBps, amountNano: entryAmount });
    basisNano = entryAmount;
  }
  return entries;
}

export type SpendCommissionOutcome = "recorded" | "duplicate" | "skipped";

// Собирает цепочку партнёров (прямой реферер → родитель → …) для расчёта комиссии.
async function loadCommissionChain(
  client: import("pg").PoolClient,
  directPartnerId: string,
): Promise<CommissionChainPartner[]> {
  const chain: CommissionChainPartner[] = [];
  let nextPartnerId: string | null = directPartnerId;
  while (nextPartnerId && chain.length <= MAX_COMMISSION_LEVELS) {
    const row: {
      rows: { id: string; status: PartnerStatus; commission_bps: number; sub_commission_bps: number; parent_partner_id: string | null }[];
    } = await client.query(
      "SELECT id, status, commission_bps, sub_commission_bps, parent_partner_id FROM partners WHERE id = $1",
      [nextPartnerId],
    );
    const partner = row.rows[0];
    if (!partner) break;
    chain.push({
      partnerId: partner.id,
      status: partner.status,
      commissionBps: partner.commission_bps,
      subCommissionBps: partner.sub_commission_bps,
    });
    nextPartnerId = partner.parent_partner_id;
  }
  return chain;
}

/**
 * Реальный депозит рефа: пишет строку в referred_topups для истории/аналитики. Комиссию НЕ создаёт
 * (комиссия капает со списаний в recordReferredSpend). Идемпотентно по commerce_payment_id.
 */
export async function recordReferredDeposit(database: SalesDatabase, input: {
  commercePaymentId: string;
  commerceUserId: string;
  amountNano: bigint;
  paidAt: Date;
}): Promise<SpendCommissionOutcome> {
  if (input.amountNano <= 0n) return "skipped";
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const referred = await client.query<{ partner_id: string }>(
      "SELECT partner_id FROM referred_users WHERE commerce_user_id = $1",
      [input.commerceUserId],
    );
    const directPartnerId = referred.rows[0]?.partner_id;
    if (!directPartnerId) {
      await client.query("ROLLBACK");
      return "skipped";
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO referred_topups (commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at)
      VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (commerce_payment_id) DO NOTHING
      RETURNING id
    `, [
      input.commercePaymentId, input.commerceUserId, directPartnerId,
      input.amountNano.toString(), input.paidAt,
    ]);
    if (!inserted.rows[0]) {
      await client.query("ROLLBACK");
      return "duplicate";
    }
    await client.query("COMMIT");
    return "recorded";
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Списание рефа (spend за API), где `amountNano` — уже РЕАЛЬНО-ОПЛАЧЕННАЯ часть списания
 * (real_funded), посчитанная коммерцией по принципу «бесплатное тратится первым»: часть из
 * $4-бонуса/промо в неё не входит и комиссию не создаёт. Сумма уже учитывает актуальную скидку/тир
 * клиента (charge = прайс × текущий мультипликатор). Награда считается по цепочке. Идемпотентно
 * по commerce_event_id.
 */
export async function recordReferredSpend(database: SalesDatabase, input: {
  commerceEventId: bigint;
  commerceUserId: string;
  amountNano: bigint; // real_funded: часть списания, покрытая реальными деньгами
  occurredAt: Date;
}): Promise<SpendCommissionOutcome> {
  if (input.amountNano <= 0n) return "skipped";
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const referred = await client.query<{ partner_id: string }>(
      "SELECT partner_id FROM referred_users WHERE commerce_user_id = $1",
      [input.commerceUserId],
    );
    const directPartnerId = referred.rows[0]?.partner_id;
    if (!directPartnerId) {
      await client.query("ROLLBACK");
      return "skipped";
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO partner_usage_events (commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at)
      VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (commerce_event_id) DO NOTHING
      RETURNING id
    `, [
      input.commerceEventId.toString(), input.commerceUserId, directPartnerId,
      input.amountNano.toString(), input.occurredAt,
    ]);
    const usageEventId = inserted.rows[0]?.id;
    if (!usageEventId) {
      await client.query("ROLLBACK");
      return "duplicate";
    }
    const chain = await loadCommissionChain(client, directPartnerId);
    for (const entry of computeCommissionChain(chain, input.amountNano)) {
      await client.query(`
        INSERT INTO commission_entries (usage_event_id, partner_id, level, applied_bps, amount_nano)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (usage_event_id, partner_id) WHERE usage_event_id IS NOT NULL DO NOTHING
      `, [usageEventId, entry.partnerId, entry.level, entry.appliedBps, entry.amountNano.toString()]);
    }
    await client.query("COMMIT");
    return "recorded";
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export interface PartnerEarningsTotals {
  earnedNano: bigint;
  directNano: bigint;
  overrideNano: bigint;
  paidNano: bigint;
  pendingPayoutNano: bigint;
  availableNano: bigint;
  last30dSpendNano: bigint;
  last30dEarnedNano: bigint;
}

export async function getPartnerEarningsTotals(database: SalesDatabase, partnerId: string): Promise<PartnerEarningsTotals> {
  const result = await database.pool.query<{
    earned: string; direct: string; override: string; paid: string; pending: string;
    spend_30d: string; earned_30d: string;
  }>(`
    SELECT
      COALESCE((SELECT SUM(amount_nano) FROM commission_entries WHERE partner_id = $1), 0)::text AS earned,
      COALESCE((SELECT SUM(amount_nano) FROM commission_entries WHERE partner_id = $1 AND level = 0), 0)::text AS direct,
      COALESCE((SELECT SUM(amount_nano) FROM commission_entries WHERE partner_id = $1 AND level > 0), 0)::text AS override,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE partner_id = $1 AND status = 'paid'), 0)::text AS paid,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE partner_id = $1 AND status IN ('requested', 'approved')), 0)::text AS pending,
      COALESCE((
        SELECT SUM(amount_nano) FROM partner_usage_events
        WHERE partner_id = $1 AND occurred_at >= now() - interval '30 days'
      ), 0)::text AS spend_30d,
      COALESCE((
        SELECT SUM(ce.amount_nano) FROM commission_entries ce
        JOIN partner_usage_events pue ON pue.id = ce.usage_event_id
        WHERE ce.partner_id = $1 AND pue.occurred_at >= now() - interval '30 days'
      ), 0)::text AS earned_30d
  `, [partnerId]);
  const row = result.rows[0]!;
  const earnedNano = BigInt(row.earned);
  const paidNano = BigInt(row.paid);
  const pendingPayoutNano = BigInt(row.pending);
  return {
    earnedNano,
    directNano: BigInt(row.direct),
    overrideNano: BigInt(row.override),
    paidNano,
    pendingPayoutNano,
    availableNano: earnedNano - paidNano - pendingPayoutNano,
    last30dSpendNano: BigInt(row.spend_30d),
    last30dEarnedNano: BigInt(row.earned_30d),
  };
}

export interface DailyEarningsPoint {
  date: string;
  spendNano: bigint;
  earnedNano: bigint;
}

export async function getPartnerDailyEarnings(
  database: SalesDatabase,
  partnerId: string,
  days: number,
): Promise<DailyEarningsPoint[]> {
  const [spendResult, earnedResult] = await Promise.all([
    database.pool.query<{ day: string; total: string }>(`
      SELECT to_char(date_trunc('day', occurred_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD') AS day,
             SUM(amount_nano)::text AS total
      FROM partner_usage_events
      WHERE partner_id = $1 AND occurred_at >= now() - ($2 * interval '1 day')
      GROUP BY 1
    `, [partnerId, days]),
    database.pool.query<{ day: string; total: string }>(`
      SELECT to_char(date_trunc('day', pue.occurred_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD') AS day,
             SUM(ce.amount_nano)::text AS total
      FROM commission_entries ce
      JOIN partner_usage_events pue ON pue.id = ce.usage_event_id
      WHERE ce.partner_id = $1 AND pue.occurred_at >= now() - ($2 * interval '1 day')
      GROUP BY 1
    `, [partnerId, days]),
  ]);
  const depositByDay = new Map(spendResult.rows.map((row) => [row.day, BigInt(row.total)]));
  const earnedByDay = new Map(earnedResult.rows.map((row) => [row.day, BigInt(row.total)]));

  const series: DailyEarningsPoint[] = [];
  const today = new Date();
  for (let offset = days - 1; offset >= 0; offset -= 1) {
    const day = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() - offset));
    const date = day.toISOString().slice(0, 10);
    series.push({
      date,
      spendNano: depositByDay.get(date) ?? 0n,
      earnedNano: earnedByDay.get(date) ?? 0n,
    });
  }
  return series;
}

export interface TeamMemberSummary {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  status: PartnerStatus;
  commissionBps: number;
  referredUsers: number;
  theirEarnedNano: bigint;
  myOverrideNano: bigint;
}

export async function listPartnerTeam(database: SalesDatabase, partnerId: string): Promise<TeamMemberSummary[]> {
  const result = await database.pool.query<{
    id: string; email: string | null; telegram_username: string | null; display_name: string | null;
    status: PartnerStatus; commission_bps: number;
    referred_users: string; their_earned: string; my_override: string;
  }>(`
    SELECT p.id, p.email, p.telegram_username, p.display_name, p.status, p.commission_bps,
      (SELECT count(*) FROM referred_users ru WHERE ru.partner_id = p.id)::text AS referred_users,
      COALESCE((SELECT SUM(ce.amount_nano) FROM commission_entries ce WHERE ce.partner_id = p.id), 0)::text AS their_earned,
      COALESCE((
        SELECT SUM(ce.amount_nano) FROM commission_entries ce
        JOIN partner_usage_events pue ON pue.id = ce.usage_event_id
        WHERE ce.partner_id = $1 AND ce.level > 0 AND pue.partner_id = p.id
      ), 0)::text AS my_override
    FROM partners p
    WHERE p.parent_partner_id = $1
    ORDER BY p.created_at
  `, [partnerId]);
  return result.rows.map((row) => ({
    id: row.id,
    email: row.email,
    telegramUsername: row.telegram_username,
    displayName: row.display_name,
    status: row.status,
    commissionBps: row.commission_bps,
    referredUsers: Number(row.referred_users),
    theirEarnedNano: BigInt(row.their_earned),
    myOverrideNano: BigInt(row.my_override),
  }));
}

export async function countPartnerTeam(database: SalesDatabase, partnerId: string): Promise<number> {
  const result = await database.pool.query<{ count: string }>(
    "SELECT count(*)::text AS count FROM partners WHERE parent_partner_id = $1",
    [partnerId],
  );
  return Number(result.rows[0]?.count ?? "0");
}
