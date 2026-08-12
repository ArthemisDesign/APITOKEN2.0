import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";
import type { PoolClient } from "pg";

export const MAX_COMMISSION_LEVELS = 10;
export const POSTGRES_BIGINT_MAX = 9_223_372_036_854_775_807n;

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

export class PartnerReferralCycleError extends Error {
  constructor(partnerId: string) {
    super(`partner referral chain contains a cycle at ${partnerId}`);
    this.name = "PartnerReferralCycleError";
  }
}

export type ReferralUsageReplayStore =
  | "v1_recorded"
  | "v1_pending"
  | "v2_recorded"
  | "v2_pending";

export interface ReferralUsageReplay {
  store: ReferralUsageReplayStore;
  rowId: string;
  commerceUserId: string;
  paidBasisNano: bigint;
  occurredAt: Date;
}

/**
 * One event id must select exactly one of the historical v1/v2 stores. The negative bijection
 * keeps this lock namespace disjoint from the existing positive process-wide sales locks while
 * preserving every non-negative PostgreSQL bigint id without hashing collisions.
 */
export async function lockReferralUsageEvent(client: PoolClient, commerceEventId: bigint): Promise<void> {
  if (commerceEventId < 0n || commerceEventId > POSTGRES_BIGINT_MAX) {
    throw new RangeError("commerce event id is outside the PostgreSQL bigint range");
  }
  const lockKey = -1n - commerceEventId;
  await client.query("SELECT pg_advisory_xact_lock($1::bigint)", [lockKey.toString()]);
}

/** Loads the common immutable identity from every usage evidence/buffer store. */
export async function loadReferralUsageReplays(
  client: PoolClient,
  commerceEventId: bigint,
): Promise<ReferralUsageReplay[]> {
  const result = await client.query<{
    store: ReferralUsageReplayStore;
    row_id: string;
    commerce_user_id: string;
    paid_basis_nano: string;
    occurred_at: Date;
  }>(`
    SELECT 'v1_recorded'::text AS store, id::text AS row_id, commerce_user_id,
           amount_nano::text AS paid_basis_nano, occurred_at
    FROM partner_usage_events
    WHERE commerce_event_id = $1
    UNION ALL
    SELECT 'v1_pending'::text AS store, id::text AS row_id, commerce_user_id,
           amount_nano::text AS paid_basis_nano, occurred_at
    FROM pending_referral_events
    WHERE kind = 'spend' AND commerce_ref = $2
    UNION ALL
    SELECT 'v2_recorded'::text AS store, id::text AS row_id, commerce_user_id,
           paid_funded_nano::text AS paid_basis_nano, occurred_at
    FROM partner_usage_events_v2
    WHERE commerce_event_id = $1
    UNION ALL
    SELECT 'v2_pending'::text AS store, id::text AS row_id, commerce_user_id,
           paid_funded_nano::text AS paid_basis_nano, occurred_at
    FROM pending_referral_usage_events_v2
    WHERE commerce_event_id = $1
    ORDER BY store
  `, [commerceEventId.toString(), commerceEventId.toString()]);
  return result.rows.map((row) => ({
    store: row.store,
    rowId: row.row_id,
    commerceUserId: row.commerce_user_id,
    paidBasisNano: BigInt(row.paid_basis_nano),
    occurredAt: row.occurred_at,
  }));
}

export function referralUsageReplayMatches(
  replay: ReferralUsageReplay,
  input: { commerceUserId: string; paidBasisNano: bigint; occurredAt: Date },
): boolean {
  return replay.commerceUserId === input.commerceUserId
    && replay.paidBasisNano === input.paidBasisNano
    && replay.occurredAt.getTime() === input.occurredAt.getTime();
}

export async function deleteReferralUsagePendingReplay(
  client: PoolClient,
  replay: ReferralUsageReplay,
): Promise<void> {
  if (replay.store === "v1_pending") {
    await client.query("DELETE FROM pending_referral_events WHERE id = $1 AND kind = 'spend'", [replay.rowId]);
  } else if (replay.store === "v2_pending") {
    await client.query("DELETE FROM pending_referral_usage_events_v2 WHERE id = $1", [replay.rowId]);
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
): Promise<string> {
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
  if (inserted.rows[0]) return inserted.rows[0].id;

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
  return stored.id;
}

// Собирает цепочку партнёров (прямой реферер → родитель → …) для расчёта комиссии.
// Экспортирована для commissions-v2: цепочка и стоп-условия общие для обеих schema.
export async function loadCommissionChain(
  client: PoolClient,
  directPartnerId: string,
): Promise<CommissionChainPartner[]> {
  const chain: CommissionChainPartner[] = [];
  const visited = new Set<string>();
  let nextPartnerId: string | null = directPartnerId;
  while (nextPartnerId) {
    if (visited.has(nextPartnerId)) throw new PartnerReferralCycleError(nextPartnerId);
    visited.add(nextPartnerId);
    const row: {
      rows: { id: string; status: PartnerStatus; commission_bps: number; sub_commission_bps: number; parent_partner_id: string | null }[];
    } = await client.query(
      `SELECT id, status, commission_bps, sub_commission_bps, parent_partner_id
       FROM partners WHERE id = $1 FOR SHARE`,
      [nextPartnerId],
    );
    const partner = row.rows[0];
    if (!partner) break;
    if (chain.length < MAX_COMMISSION_LEVELS) {
      chain.push({
        partnerId: partner.id,
        status: partner.status,
        commissionBps: partner.commission_bps,
        subCommissionBps: partner.sub_commission_bps,
      });
    }
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
    const referred = await client.query<{ partner_id: string; attributed_at: Date }>(
      "SELECT partner_id, attributed_at FROM referred_users WHERE commerce_user_id = $1",
      [input.commerceUserId],
    );
    const referredUser = referred.rows[0];
    const directPartnerId = referredUser?.partner_id;
    type StoredDeposit = {
      commerce_user_id: string;
      partner_id: string;
      amount_nano: string;
      paid_at: Date;
    };
    const loadStoredDeposit = async (): Promise<StoredDeposit | undefined> => {
      const existing = await client.query<StoredDeposit>(`
        SELECT commerce_user_id, partner_id, amount_nano::text AS amount_nano, paid_at
        FROM referred_topups
        WHERE commerce_payment_id = $1
        FOR UPDATE
      `, [input.commercePaymentId]);
      return existing.rows[0];
    };
    const storedDepositMatches = (stored: StoredDeposit): boolean => directPartnerId !== undefined
      && stored.commerce_user_id === input.commerceUserId
      && stored.partner_id === directPartnerId
      && stored.amount_nano === input.amountNano.toString()
      && stored.paid_at.getTime() === input.paidAt.getTime();

    const previousDeposit = await loadStoredDeposit();
    if (previousDeposit) {
      if (
        !storedDepositMatches(previousDeposit)
        || !referredUser
        || input.paidAt.getTime() < referredUser.attributed_at.getTime()
      ) {
        throw new ReferralEventReplayConflictError("deposit", input.commercePaymentId);
      }
      await client.query("COMMIT");
      return "duplicate";
    }
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
    if (input.paidAt.getTime() < referredUser.attributed_at.getTime()) {
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
      const stored = await loadStoredDeposit();
      if (!stored || !storedDepositMatches(stored)) {
        throw new ReferralEventReplayConflictError("deposit", input.commercePaymentId);
      }
      await client.query("COMMIT");
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
    await lockReferralUsageEvent(client, input.commerceEventId);
    const replays = await loadReferralUsageReplays(client, input.commerceEventId);
    if (replays.length > 1) {
      throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
    }
    const replay = replays[0];
    if (replay && !referralUsageReplayMatches(replay, {
      commerceUserId: input.commerceUserId,
      paidBasisNano: input.amountNano,
      occurredAt: input.occurredAt,
    })) {
      throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
    }

    const referred = await client.query<{ partner_id: string; attributed_at: Date }>(
      `SELECT partner_id, attributed_at
       FROM referred_users WHERE commerce_user_id = $1 FOR SHARE`,
      [input.commerceUserId],
    );
    const referredUser = referred.rows[0];
    const directPartnerId = referredUser?.partner_id;

    if (replay?.store === "v2_recorded" || replay?.store === "v2_pending") {
      if (!directPartnerId && replay.store === "v2_recorded") {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }
      if (referredUser && input.occurredAt.getTime() < referredUser.attributed_at.getTime()) {
        if (replay.store === "v2_recorded") {
          throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
        }
        await deleteReferralUsagePendingReplay(client, replay);
        await client.query("COMMIT");
        return "skipped";
      }
      await client.query("COMMIT");
      return replay.store === "v2_recorded" ? "duplicate" : "buffered";
    }

    if (replay?.store === "v1_recorded") {
      if (!directPartnerId) {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }
      const existing = await client.query<StoredSpendAttribution & {
        commerce_user_id: string;
        partner_id: string;
        amount_nano: string;
        occurred_at: Date;
      }>(`
        SELECT commerce_user_id, partner_id, amount_nano::text AS amount_nano, occurred_at,
               provider_id, account_class, pricing_mode,
               paid_funded_nano::text AS paid_funded_nano,
               commission_eligible, snapshot_digest
        FROM partner_usage_events
        WHERE commerce_event_id = $1
        FOR UPDATE
      `, [input.commerceEventId.toString()]);
      let stored = existing.rows[0];
      if (
        !stored
        || stored.commerce_user_id !== input.commerceUserId
        || stored.partner_id !== directPartnerId
        || stored.amount_nano !== input.amountNano.toString()
        || stored.occurred_at.getTime() !== input.occurredAt.getTime()
      ) {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }
      if (input.occurredAt.getTime() < referredUser.attributed_at.getTime()) {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }
      if (attribution !== null && storedAttributionIsNull(stored)) {
        const upgraded = await client.query<StoredSpendAttribution & {
          commerce_user_id: string;
          partner_id: string;
          amount_nano: string;
          occurred_at: Date;
        }>(`
          UPDATE partner_usage_events
          SET provider_id = $2, account_class = $3, pricing_mode = $4, paid_funded_nano = $5,
              commission_eligible = $6, snapshot_digest = $7
          WHERE commerce_event_id = $1 AND provider_id IS NULL
          RETURNING commerce_user_id, partner_id, amount_nano::text AS amount_nano, occurred_at,
                    provider_id, account_class, pricing_mode,
                    paid_funded_nano::text AS paid_funded_nano,
                    commission_eligible, snapshot_digest
        `, [input.commerceEventId.toString(), ...attributionSqlValues(attribution)]);
        stored = upgraded.rows[0] ?? stored;
      }
      if (!storedAttributionMatches(stored, attribution)) {
        throw new ReferralEventReplayConflictError("spend", input.commerceEventId.toString());
      }
      await client.query("COMMIT");
      return "duplicate";
    }

    let pendingReferralEventId: string | null = null;
    if (replay?.store === "v1_pending") {
      pendingReferralEventId = await bufferPendingReferralEvent(
        client,
        "spend",
        input.commerceEventId.toString(),
        input.commerceUserId,
        input.amountNano,
        input.occurredAt,
        attribution,
      );
    }

    if (!directPartnerId) {
      // Спенд пришёл раньше атрибуции юзера — буферизуем, reconcile проиграет после атрибуции.
      if (pendingReferralEventId === null) {
        await bufferPendingReferralEvent(
          client,
          "spend",
          input.commerceEventId.toString(),
          input.commerceUserId,
          input.amountNano,
          input.occurredAt,
          attribution,
        );
      }
      await client.query("COMMIT");
      return "buffered";
    }
    if (input.occurredAt.getTime() < referredUser.attributed_at.getTime()) {
      if (replay) await deleteReferralUsagePendingReplay(client, replay);
      await client.query("COMMIT");
      return "skipped";
    }
    if (pendingReferralEventId !== null) {
      await client.query(
        "DELETE FROM pending_referral_events WHERE id = $1 AND kind = 'spend'",
        [pendingReferralEventId],
      );
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
    const usageEventId = inserted.rows[0]?.id;
    if (!usageEventId) {
      // Mixed-version safety: a previous binary may not take the advisory lock. Validate its
      // winner exactly rather than treating every same-schema conflict as an idempotent replay.
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
  /** Immutable positive commission history. */
  earnedNano: bigint;
  directNano: bigint;
  overrideNano: bigint;
  /** Signed refund/dispute corrections; always zero or negative. */
  adjustmentNano: bigint;
  directAdjustmentNano: bigint;
  overrideAdjustmentNano: bigint;
  netNano: bigint;
  directNetNano: bigint;
  overrideNetNano: bigint;
  paidNano: bigint;
  pendingPayoutNano: bigint;
  debtNano: bigint;
  availableNano: bigint;
  last30dSpendNano: bigint;
  /** Gross is retained for audit/API compatibility; net is what is actually owed. */
  last30dEarnedNano: bigint;
  last30dAdjustmentNano: bigint;
  last30dNetNano: bigint;
}

export async function getPartnerEarningsTotals(database: SalesDatabase, partnerId: string): Promise<PartnerEarningsTotals> {
  // Комиссии и спенд агрегируются по ОБЕИМ schema (v1 commission_entries/partner_usage_events и
  // v2 commission_entries_v2/partner_usage_events_v2): события между ними не пересекаются, поэтому
  // UNION ALL не даёт двойного счёта. V2-спенд — exact paid_funded_nano (там нет amount_nano).
  const result = await database.pool.query<{
    earned: string; direct: string; override: string;
    adjustment: string; direct_adjustment: string; override_adjustment: string;
    paid: string; pending: string; spend_30d: string; earned_30d: string;
    adjustment_30d: string;
  }>(`
    WITH all_commissions AS (
      SELECT partner_id, level, amount_nano FROM commission_entries
      UNION ALL
      SELECT partner_id, level, amount_nano FROM commission_entries_v2
    ), all_usage AS (
      SELECT partner_id, amount_nano, occurred_at FROM partner_usage_events
      UNION ALL
      SELECT partner_id, paid_funded_nano, occurred_at FROM partner_usage_events_v2
    ), all_earned_by_usage AS (
      SELECT ce.partner_id, ce.amount_nano, pue.occurred_at
      FROM commission_entries ce
      JOIN partner_usage_events pue ON pue.id = ce.usage_event_id
      UNION ALL
      SELECT ce.partner_id, ce.amount_nano, pue.occurred_at
      FROM commission_entries_v2 ce
      JOIN partner_usage_events_v2 pue ON pue.id = ce.usage_event_id
    ), all_adjustments AS (
      SELECT adjustment.partner_id, adjustment.amount_nano, adjustment.effective_at,
             COALESCE(entry.level, entry_v2.level) AS level
      FROM partner_commission_adjustments adjustment
      JOIN partner_commission_funding_allocations allocation
        ON allocation.id = adjustment.commission_funding_allocation_id
      LEFT JOIN commission_entries entry ON entry.id = allocation.commission_entry_id
      LEFT JOIN commission_entries_v2 entry_v2 ON entry_v2.id = allocation.commission_entry_v2_id
    )
    SELECT
      COALESCE((SELECT SUM(amount_nano) FROM all_commissions WHERE partner_id = $1), 0)::text AS earned,
      COALESCE((SELECT SUM(amount_nano) FROM all_commissions WHERE partner_id = $1 AND level = 0), 0)::text AS direct,
      COALESCE((SELECT SUM(amount_nano) FROM all_commissions WHERE partner_id = $1 AND level > 0), 0)::text AS override,
      COALESCE((SELECT SUM(amount_nano) FROM all_adjustments WHERE partner_id = $1), 0)::text AS adjustment,
      COALESCE((SELECT SUM(amount_nano) FROM all_adjustments WHERE partner_id = $1 AND level = 0), 0)::text AS direct_adjustment,
      COALESCE((SELECT SUM(amount_nano) FROM all_adjustments WHERE partner_id = $1 AND level > 0), 0)::text AS override_adjustment,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE partner_id = $1 AND status = 'paid'), 0)::text AS paid,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE partner_id = $1 AND status IN ('requested', 'approved')), 0)::text AS pending,
      COALESCE((
        SELECT SUM(amount_nano) FROM all_usage
        WHERE partner_id = $1 AND occurred_at >= now() - interval '30 days'
      ), 0)::text AS spend_30d,
      COALESCE((
        SELECT SUM(amount_nano) FROM all_earned_by_usage
        WHERE partner_id = $1 AND occurred_at >= now() - interval '30 days'
      ), 0)::text AS earned_30d,
      COALESCE((
        SELECT SUM(amount_nano) FROM all_adjustments
        WHERE partner_id = $1 AND effective_at >= now() - interval '30 days'
      ), 0)::text AS adjustment_30d
  `, [partnerId]);
  const row = result.rows[0]!;
  const earnedNano = BigInt(row.earned);
  const directNano = BigInt(row.direct);
  const overrideNano = BigInt(row.override);
  const adjustmentNano = BigInt(row.adjustment);
  const directAdjustmentNano = BigInt(row.direct_adjustment);
  const overrideAdjustmentNano = BigInt(row.override_adjustment);
  const netNano = earnedNano + adjustmentNano;
  const paidNano = BigInt(row.paid);
  const pendingPayoutNano = BigInt(row.pending);
  const availableBalance = netNano - paidNano - pendingPayoutNano;
  const paidBalance = netNano - paidNano;
  const last30dEarnedNano = BigInt(row.earned_30d);
  const last30dAdjustmentNano = BigInt(row.adjustment_30d);
  return {
    earnedNano,
    directNano,
    overrideNano,
    adjustmentNano,
    directAdjustmentNano,
    overrideAdjustmentNano,
    netNano,
    directNetNano: directNano + directAdjustmentNano,
    overrideNetNano: overrideNano + overrideAdjustmentNano,
    paidNano,
    pendingPayoutNano,
    debtNano: paidBalance < 0n ? -paidBalance : 0n,
    availableNano: availableBalance > 0n ? availableBalance : 0n,
    last30dSpendNano: BigInt(row.spend_30d),
    last30dEarnedNano,
    last30dAdjustmentNano,
    last30dNetNano: last30dEarnedNano + last30dAdjustmentNano,
  };
}

export interface DailyEarningsPoint {
  date: string;
  spendNano: bigint;
  /** Gross positive commissions retained for audit compatibility. */
  earnedNano: bigint;
  adjustmentNano: bigint;
  netNano: bigint;
}

export async function getPartnerDailyEarnings(
  database: SalesDatabase,
  partnerId: string,
  days: number,
): Promise<DailyEarningsPoint[]> {
  const [spendResult, earnedResult, adjustmentResult] = await Promise.all([
    database.pool.query<{ day: string; total: string }>(`
      SELECT to_char(date_trunc('day', occurred_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD') AS day,
             SUM(amount_nano)::text AS total
      FROM (
        SELECT partner_id, amount_nano, occurred_at FROM partner_usage_events
        UNION ALL
        SELECT partner_id, paid_funded_nano, occurred_at FROM partner_usage_events_v2
      ) all_usage
      WHERE partner_id = $1 AND occurred_at >= now() - ($2 * interval '1 day')
      GROUP BY 1
    `, [partnerId, days]),
    database.pool.query<{ day: string; total: string }>(`
      SELECT to_char(date_trunc('day', occurred_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD') AS day,
             SUM(amount_nano)::text AS total
      FROM (
        SELECT ce.partner_id, ce.amount_nano, pue.occurred_at
        FROM commission_entries ce
        JOIN partner_usage_events pue ON pue.id = ce.usage_event_id
        UNION ALL
        SELECT ce.partner_id, ce.amount_nano, pue.occurred_at
        FROM commission_entries_v2 ce
        JOIN partner_usage_events_v2 pue ON pue.id = ce.usage_event_id
      ) all_earned
      WHERE partner_id = $1 AND occurred_at >= now() - ($2 * interval '1 day')
      GROUP BY 1
    `, [partnerId, days]),
    database.pool.query<{ day: string; total: string }>(`
      SELECT to_char(date_trunc('day', effective_at AT TIME ZONE 'UTC'), 'YYYY-MM-DD') AS day,
             SUM(amount_nano)::text AS total
      FROM partner_commission_adjustments
      WHERE partner_id = $1 AND effective_at >= now() - ($2 * interval '1 day')
      GROUP BY 1
    `, [partnerId, days]),
  ]);
  const depositByDay = new Map(spendResult.rows.map((row) => [row.day, BigInt(row.total)]));
  const earnedByDay = new Map(earnedResult.rows.map((row) => [row.day, BigInt(row.total)]));
  const adjustmentByDay = new Map(adjustmentResult.rows.map((row) => [row.day, BigInt(row.total)]));

  const series: DailyEarningsPoint[] = [];
  const today = new Date();
  for (let offset = days - 1; offset >= 0; offset -= 1) {
    const day = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() - offset));
    const date = day.toISOString().slice(0, 10);
    const earnedNano = earnedByDay.get(date) ?? 0n;
    const adjustmentNano = adjustmentByDay.get(date) ?? 0n;
    series.push({
      date,
      spendNano: depositByDay.get(date) ?? 0n,
      earnedNano,
      adjustmentNano,
      netNano: earnedNano + adjustmentNano,
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
  theirAdjustmentNano: bigint;
  theirNetNano: bigint;
  myOverrideNano: bigint;
  myOverrideAdjustmentNano: bigint;
  myOverrideNetNano: bigint;
}

export async function listPartnerTeam(database: SalesDatabase, partnerId: string): Promise<TeamMemberSummary[]> {
  const result = await database.pool.query<{
    id: string; email: string | null; telegram_username: string | null; display_name: string | null;
    status: PartnerStatus; commission_bps: number;
    referred_users: string; their_earned: string; their_adjustment: string;
    my_override: string; my_override_adjustment: string;
  }>(`
    SELECT p.id, p.email, p.telegram_username, p.display_name, p.status, p.commission_bps,
      (SELECT count(*) FROM referred_users ru WHERE ru.partner_id = p.id)::text AS referred_users,
      COALESCE((
        SELECT SUM(amount_nano) FROM (
          SELECT partner_id, amount_nano FROM commission_entries
          UNION ALL
          SELECT partner_id, amount_nano FROM commission_entries_v2
        ) ce WHERE ce.partner_id = p.id
      ), 0)::text AS their_earned,
      COALESCE((
        SELECT SUM(amount_nano) FROM partner_commission_adjustments adjustment
        WHERE adjustment.partner_id = p.id
      ), 0)::text AS their_adjustment,
      COALESCE((
        SELECT SUM(amount_nano) FROM (
          SELECT ce.partner_id, ce.level, ce.amount_nano, pue.partner_id AS source_partner_id
          FROM commission_entries ce
          JOIN partner_usage_events pue ON pue.id = ce.usage_event_id
          UNION ALL
          SELECT ce.partner_id, ce.level, ce.amount_nano, pue.partner_id
          FROM commission_entries_v2 ce
          JOIN partner_usage_events_v2 pue ON pue.id = ce.usage_event_id
        ) override_entries
        WHERE override_entries.partner_id = $1 AND override_entries.level > 0
          AND override_entries.source_partner_id = p.id
      ), 0)::text AS my_override,
      COALESCE((
        SELECT SUM(adjustment.amount_nano)
        FROM partner_commission_adjustments adjustment
        JOIN partner_commission_funding_allocations allocation
          ON allocation.id = adjustment.commission_funding_allocation_id
        LEFT JOIN commission_entries entry ON entry.id = allocation.commission_entry_id
        LEFT JOIN commission_entries_v2 entry_v2 ON entry_v2.id = allocation.commission_entry_v2_id
        LEFT JOIN partner_usage_events usage ON usage.id = entry.usage_event_id
        LEFT JOIN partner_usage_events_v2 usage_v2 ON usage_v2.id = entry_v2.usage_event_id
        WHERE adjustment.partner_id = $1
          AND COALESCE(entry.level, entry_v2.level) > 0
          AND COALESCE(usage.partner_id, usage_v2.partner_id) = p.id
      ), 0)::text AS my_override_adjustment
    FROM partners p
    WHERE p.parent_partner_id = $1
    ORDER BY p.created_at
  `, [partnerId]);
  return result.rows.map((row) => {
    const theirEarnedNano = BigInt(row.their_earned);
    const theirAdjustmentNano = BigInt(row.their_adjustment);
    const myOverrideNano = BigInt(row.my_override);
    const myOverrideAdjustmentNano = BigInt(row.my_override_adjustment);
    return {
    id: row.id,
    email: row.email,
    telegramUsername: row.telegram_username,
    displayName: row.display_name,
    status: row.status,
    commissionBps: row.commission_bps,
    referredUsers: Number(row.referred_users),
      theirEarnedNano,
      theirAdjustmentNano,
      theirNetNano: theirEarnedNano + theirAdjustmentNano,
      myOverrideNano,
      myOverrideAdjustmentNano,
      myOverrideNetNano: myOverrideNano + myOverrideAdjustmentNano,
    };
  });
}

export async function countPartnerTeam(database: SalesDatabase, partnerId: string): Promise<number> {
  const result = await database.pool.query<{ count: string }>(
    "SELECT count(*)::text AS count FROM partners WHERE parent_partner_id = $1",
    [partnerId],
  );
  return Number(result.rows[0]?.count ?? "0");
}
