import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";
import type { PoolClient } from "pg";

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
 * `previousLevelAmount * subCommissionBps / 10000`. The walk stops at the first NON-active
 * partner (pending or suspended — no entry for it, chain ends there), when a computed amount
 * reaches 0, or after exactly MAX_COMMISSION_LEVELS levels (0..MAX-1).
 */
export function computeCommissionChain(
  partnersChain: readonly CommissionChainPartner[],
  amountNano: bigint,
): CommissionEntryPlan[] {
  const entries: CommissionEntryPlan[] = [];
  if (amountNano <= 0n) return entries;
  let basisNano = amountNano;
  for (let level = 0; level < partnersChain.length && level < MAX_COMMISSION_LEVELS; level += 1) {
    const partner = partnersChain[level]!;
    if (partner.status !== "active") break;
    const appliedBps = level === 0 ? partner.commissionBps : partner.subCommissionBps;
    const entryAmount = (basisNano * BigInt(appliedBps)) / 10_000n;
    if (entryAmount <= 0n) break;
    entries.push({ partnerId: partner.partnerId, level, appliedBps, amountNano: entryAmount });
    basisNano = entryAmount;
  }
  return entries;
}

export type SpendCommissionOutcome = "recorded" | "duplicate" | "skipped" | "buffered";

export interface ReferredSpendAttribution {
  providerId: string;
  accountClass: "b2c";
  pricingMode: "track";
  paidFundedNano: bigint;
  commissionEligible: true;
  snapshotDigest: string;
}

export class ReferredSpendAttributionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ReferredSpendAttributionError";
  }
}

export class ReferralEventReplayConflictError extends Error {
  constructor(kind: "spend" | "deposit", commerceRef: string) {
    super(`${kind} referral event ${commerceRef} conflicts with its immutable replay`);
    this.name = "ReferralEventReplayConflictError";
  }
}

interface StoredSpendAttribution {
  provider_id: string | null;
  account_class: string | null;
  pricing_mode: string | null;
  paid_funded_nano: string | null;
  commission_eligible: boolean | null;
  snapshot_digest: string | null;
}

function assertValidSpendAttribution(
  amountNano: bigint,
  attribution: ReferredSpendAttribution | null,
): void {
  if (attribution === null) return;
  if (
    typeof attribution.providerId !== "string"
    || attribution.providerId.length === 0
    || attribution.accountClass !== "b2c"
    || attribution.pricingMode !== "track"
    || typeof attribution.paidFundedNano !== "bigint"
    || attribution.paidFundedNano <= 0n
    || attribution.paidFundedNano !== amountNano
    || attribution.commissionEligible !== true
    || typeof attribution.snapshotDigest !== "string"
    || attribution.snapshotDigest.length === 0
  ) {
    throw new ReferredSpendAttributionError(
      "attributed commission requires complete B2C track identity and exact positive paid funding",
    );
  }
}

function storedAttributionMatches(
  stored: StoredSpendAttribution,
  attribution: ReferredSpendAttribution | null,
): boolean {
  // An older producer/consumer may replay an event after a newer replay enriched its previously
  // null attribution. Never downgrade the stored authority, but keep that replay idempotent.
  if (attribution === null) return true;
  return stored.provider_id === attribution.providerId
    && stored.account_class === attribution.accountClass
    && stored.pricing_mode === attribution.pricingMode
    && stored.paid_funded_nano === attribution.paidFundedNano.toString()
    && stored.commission_eligible === attribution.commissionEligible
    && stored.snapshot_digest === attribution.snapshotDigest;
}

function storedAttributionIsNull(stored: StoredSpendAttribution): boolean {
  return stored.provider_id === null
    && stored.account_class === null
    && stored.pricing_mode === null
    && stored.paid_funded_nano === null
    && stored.commission_eligible === null
    && stored.snapshot_digest === null;
}

function attributionSqlValues(attribution: ReferredSpendAttribution | null): Array<string | boolean | null> {
  return attribution === null
    ? [null, null, null, null, null, null]
    : [
        attribution.providerId,
        attribution.accountClass,
        attribution.pricingMode,
        attribution.paidFundedNano.toString(),
        attribution.commissionEligible,
        attribution.snapshotDigest,
      ];
}

// Буферизует событие, пришедшее раньше атрибуции пользователя, чтобы reconcile проиграл его позже.
// Идемпотентно по (kind, commerce_ref). Использует уже открытую транзакцию клиента.
async function bufferPendingReferralEvent(
  client: PoolClient,
  kind: "spend" | "deposit",
  commerceRef: string,
  commerceUserId: string,
  amountNano: bigint,
  occurredAt: Date,
  attribution: ReferredSpendAttribution | null,
): Promise<void> {
  const attributionValues = attributionSqlValues(attribution);
  const inserted = await client.query<{ id: string }>(`
    INSERT INTO pending_referral_events (
      kind, commerce_ref, commerce_user_id, amount_nano, occurred_at,
      provider_id, account_class, pricing_mode, paid_funded_nano,
      commission_eligible, snapshot_digest
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
    ON CONFLICT (kind, commerce_ref) DO NOTHING
    RETURNING id
  `, [kind, commerceRef, commerceUserId, amountNano.toString(), occurredAt, ...attributionValues]);
  if (inserted.rows[0]) return;

  const existing = await client.query<StoredSpendAttribution & {
    id: string;
    commerce_user_id: string;
    amount_nano: string;
    occurred_at: Date;
  }>(`
    SELECT id, commerce_user_id, amount_nano::text AS amount_nano, occurred_at,
           provider_id, account_class, pricing_mode, paid_funded_nano::text AS paid_funded_nano,
           commission_eligible, snapshot_digest
    FROM pending_referral_events
    WHERE kind = $1 AND commerce_ref = $2
    FOR UPDATE
  `, [kind, commerceRef]);
  let stored = existing.rows[0];
  if (!stored) throw new ReferralEventReplayConflictError(kind, commerceRef);

  const sameBase = stored.commerce_user_id === commerceUserId
    && stored.amount_nano === amountNano.toString()
    && stored.occurred_at.getTime() === occurredAt.getTime();
  if (!sameBase) throw new ReferralEventReplayConflictError(kind, commerceRef);

  // Rolling deployment: a retry may be the first copy that carries immutable attribution. Enrich
  // an all-null legacy buffer exactly once; attributed rows can never be overwritten.
  if (attribution !== null && storedAttributionIsNull(stored)) {
    const upgraded = await client.query<StoredSpendAttribution & {
      id: string;
      commerce_user_id: string;
      amount_nano: string;
      occurred_at: Date;
    }>(`
      UPDATE pending_referral_events
      SET provider_id = $2, account_class = $3, pricing_mode = $4, paid_funded_nano = $5,
          commission_eligible = $6, snapshot_digest = $7
      WHERE id = $1 AND provider_id IS NULL
      RETURNING id, commerce_user_id, amount_nano::text AS amount_nano, occurred_at,
                provider_id, account_class, pricing_mode,
                paid_funded_nano::text AS paid_funded_nano,
                commission_eligible, snapshot_digest
    `, [stored.id, ...attributionSqlValues(attribution)]);
    stored = upgraded.rows[0] ?? stored;
  }
  if (!storedAttributionMatches(stored, attribution)) {
    throw new ReferralEventReplayConflictError(kind, commerceRef);
  }
}

// Собирает цепочку партнёров (прямой реферер → родитель → …) для расчёта комиссии.
async function loadCommissionChain(
  client: PoolClient,
  directPartnerId: string,
): Promise<CommissionChainPartner[]> {
  const chain: CommissionChainPartner[] = [];
  let nextPartnerId: string | null = directPartnerId;
  while (nextPartnerId && chain.length < MAX_COMMISSION_LEVELS) {
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
      // Депозит пришёл раньше атрибуции юзера — буферизуем, reconcile проиграет после атрибуции.
      await bufferPendingReferralEvent(
        client,
        "deposit",
        input.commercePaymentId,
        input.commerceUserId,
        input.amountNano,
        input.paidAt,
        null,
      );
      await client.query("COMMIT");
      return "buffered";
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
 * Списание рефа (spend за API). Для новых policy_v1 событий `amountNano` обязан точно совпадать
 * с immutable paid_funded_nano B2C track attribution; это единственная комиссия-authority.
 * `attribution=null` остаётся временным legacy free-first путём для старых ledger rows.
 * Награда считается по цепочке и идемпотентна по commerce_event_id.
 */
export async function recordReferredSpend(database: SalesDatabase, input: {
  commerceEventId: bigint;
  commerceUserId: string;
  amountNano: bigint;
  attribution?: ReferredSpendAttribution | null;
  occurredAt: Date;
}): Promise<SpendCommissionOutcome> {
  const attribution = input.attribution ?? null;
  assertValidSpendAttribution(input.amountNano, attribution);
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
      // Спенд пришёл раньше атрибуции юзера — буферизуем, reconcile проиграет после атрибуции.
      await bufferPendingReferralEvent(
        client,
        "spend",
        input.commerceEventId.toString(),
        input.commerceUserId,
        input.amountNano,
        input.occurredAt,
        attribution,
      );
      await client.query("COMMIT");
      return "buffered";
    }
    const attributionValues = attributionSqlValues(attribution);
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO partner_usage_events (
        commerce_event_id, commerce_user_id, partner_id, amount_nano,
        provider_id, account_class, pricing_mode, paid_funded_nano,
        commission_eligible, snapshot_digest, occurred_at
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
      ON CONFLICT (commerce_event_id) DO NOTHING
      RETURNING id
    `, [
      input.commerceEventId.toString(), input.commerceUserId, directPartnerId,
      input.amountNano.toString(), ...attributionValues, input.occurredAt,
    ]);
    let usageEventId = inserted.rows[0]?.id;
    if (!usageEventId) {
      const existing = await client.query<StoredSpendAttribution & {
        id: string;
        commerce_user_id: string;
        partner_id: string;
        amount_nano: string;
        occurred_at: Date;
      }>(`
        SELECT id, commerce_user_id, partner_id, amount_nano::text AS amount_nano, occurred_at,
               provider_id, account_class, pricing_mode,
               paid_funded_nano::text AS paid_funded_nano,
               commission_eligible, snapshot_digest
        FROM partner_usage_events
        WHERE commerce_event_id = $1
        FOR UPDATE
      `, [input.commerceEventId.toString()]);
      let stored = existing.rows[0];
      const sameBase = stored
        && stored.commerce_user_id === input.commerceUserId
        && stored.partner_id === directPartnerId
        && stored.amount_nano === input.amountNano.toString()
        && stored.occurred_at.getTime() === input.occurredAt.getTime();
      if (!stored || !sameBase) {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }

      // Preserve a stronger replay without allowing attributed evidence to be changed later.
      if (attribution !== null && storedAttributionIsNull(stored)) {
        const upgraded = await client.query<StoredSpendAttribution & {
          id: string;
          commerce_user_id: string;
          partner_id: string;
          amount_nano: string;
          occurred_at: Date;
        }>(`
          UPDATE partner_usage_events
          SET provider_id = $2, account_class = $3, pricing_mode = $4, paid_funded_nano = $5,
              commission_eligible = $6, snapshot_digest = $7
          WHERE id = $1 AND provider_id IS NULL
          RETURNING id, commerce_user_id, partner_id, amount_nano::text AS amount_nano, occurred_at,
                    provider_id, account_class, pricing_mode,
                    paid_funded_nano::text AS paid_funded_nano,
                    commission_eligible, snapshot_digest
        `, [stored.id, ...attributionSqlValues(attribution)]);
        stored = upgraded.rows[0] ?? stored;
      }
      if (!storedAttributionMatches(stored, attribution)) {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }
      await client.query("COMMIT");
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

/**
 * Проигрывает буфер pending_referral_events для пользователей, которые УЖЕ появились в referred_users
 * (атрибуция пришла позже события). Каждое событие проигрывается идемпотентно через record*-функции;
 * успешно обработанное (recorded/duplicate/skipped) удаляется из буфера, «buffered» (юзер всё ещё не
 * атрибутирован — гонка) оставляется на следующий тик. Возвращает число обработанных строк.
 */
export async function reconcilePendingReferralEvents(database: SalesDatabase, limit = 200): Promise<number> {
  const pending = await database.pool.query<StoredSpendAttribution & {
    id: string;
    kind: "spend" | "deposit";
    commerce_ref: string;
    commerce_user_id: string;
    amount_nano: string;
    occurred_at: Date;
  }>(`
    SELECT pe.id, pe.kind, pe.commerce_ref, pe.commerce_user_id,
           pe.amount_nano::text AS amount_nano, pe.occurred_at,
           pe.provider_id, pe.account_class, pe.pricing_mode,
           pe.paid_funded_nano::text AS paid_funded_nano,
           pe.commission_eligible, pe.snapshot_digest
    FROM pending_referral_events pe
    JOIN referred_users ru ON ru.commerce_user_id = pe.commerce_user_id
    ORDER BY pe.id
    LIMIT $1
  `, [limit]);

  let processed = 0;
  for (const row of pending.rows) {
    let outcome: SpendCommissionOutcome;
    if (row.kind === "spend") {
      const amountNano = BigInt(row.amount_nano);
      let attribution: ReferredSpendAttribution | null = null;
      if (!storedAttributionIsNull(row)) {
        if (
          row.provider_id === null
          || row.account_class !== "b2c"
          || row.pricing_mode !== "track"
          || row.paid_funded_nano === null
          || row.commission_eligible !== true
          || row.snapshot_digest === null
        ) throw new ReferredSpendAttributionError("buffered usage attribution is incomplete");
        attribution = {
          providerId: row.provider_id,
          accountClass: row.account_class,
          pricingMode: row.pricing_mode,
          paidFundedNano: BigInt(row.paid_funded_nano),
          commissionEligible: row.commission_eligible,
          snapshotDigest: row.snapshot_digest,
        };
      }
      outcome = await recordReferredSpend(database, {
          commerceEventId: BigInt(row.commerce_ref),
          commerceUserId: row.commerce_user_id,
          amountNano,
          attribution,
          occurredAt: row.occurred_at,
        });
    } else {
      outcome = await recordReferredDeposit(database, {
          commercePaymentId: row.commerce_ref,
          commerceUserId: row.commerce_user_id,
          amountNano: BigInt(row.amount_nano),
          paidAt: row.occurred_at,
        });
    }
    // «buffered» = юзер снова оказался неатрибутирован (крайне редкая гонка) — не удаляем, повторим позже.
    if (outcome !== "buffered") {
      await database.pool.query("DELETE FROM pending_referral_events WHERE id = $1", [row.id]);
      processed += 1;
    }
  }
  return processed;
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
