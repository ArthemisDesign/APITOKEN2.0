import type { SalesDatabase } from "./client.js";
import {
  computeCommissionChain,
  loadCommissionChain,
  type SpendCommissionOutcome,
} from "./commissions.js";

// Schema-v2 writer (миграция 0015): referred-B2C release_v2 usage-события из internal sales feed.
// Commission basis — СТРОГО exact paid_funded_nano; bonus/other-funded части никогда не
// комиссионируются. V1 и v2 события не пересекаются (одно commerce-событие ровно в одной schema),
// поэтому читатели агрегируют обе таблицы UNION ALL без двойного счёта.

export interface ReferredSpendV2Event {
  commerceEventId: bigint;
  commerceUserId: string;
  providerId: string;
  accountClass: "b2c";
  officialNano: bigint;
  chargedNano: bigint;
  paidFundedNano: bigint;
  bonusFundedNano: bigint;
  otherFundedNano: bigint;
  commissionEligible: boolean;
  releaseGeneration: bigint;
  releaseDigest: string;
  snapshotDigest: string;
  occurredAt: Date;
}

export class ReferredSpendV2ShapeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ReferredSpendV2ShapeError";
  }
}

/**
 * Структурная валидация v2-события ДО записи: зеркалит CHECK `partner_usage_events_v2_shape_check`,
 * чтобы невалидная строка упала понятной ошибкой, а не голым 23514 из БД. Нарушение — баг
 * producer'а; writer бросает и страница фида останавливается до исправления (fail closed).
 */
export function assertReferredSpendV2Shape(event: ReferredSpendV2Event): void {
  const amounts = [
    event.officialNano, event.chargedNano, event.paidFundedNano,
    event.bonusFundedNano, event.otherFundedNano,
  ];
  if (
    typeof event.providerId !== "string"
    || event.providerId.length === 0
    || event.accountClass !== "b2c"
    || amounts.some((value) => typeof value !== "bigint" || value < 0n)
    || event.paidFundedNano + event.bonusFundedNano + event.otherFundedNano !== event.chargedNano
    || typeof event.releaseGeneration !== "bigint"
    || event.releaseGeneration <= 0n
    || typeof event.releaseDigest !== "string"
    || event.releaseDigest.length === 0
    || typeof event.snapshotDigest !== "string"
    || event.snapshotDigest.length === 0
  ) {
    throw new ReferredSpendV2ShapeError(
      "release-v2 usage event must carry the complete referred-B2C funding lineage",
    );
  }
}

/** Комиссионность v2-события: только eligible строка с положительным exact paid funding. */
export function isCommissionableSpendV2(event: ReferredSpendV2Event): boolean {
  return event.commissionEligible === true && event.paidFundedNano > 0n;
}

/** Детерминированный ref буфера: одно commerce-событие — ровно одна pending-строка. */
export function pendingUsageV2Ref(commerceEventId: bigint): string {
  return `usage-v2:${commerceEventId.toString()}`;
}

/**
 * Списание рефа по schema v2. Одна транзакция: immutable usage event + цепочка комиссий
 * level 0..10 (basis = exact paid_funded_nano; trigger `commission_entries_v2_source_guard`
 * отклонит любую строку вне активной цепочки — это fail-closed вторая линия).
 * Идемпотентно по commerce_event_id (повтор — "duplicate" без второй записи).
 * Юзер ещё не атрибутирован — событие буферизуется в pending_referral_usage_events_v2
 * ("buffered"), reconcile проиграет после атрибуции. Навсегда ineligible (commission_eligible
 * = false или paid_funded_nano <= 0) — "skipped": комиссия из такого события невозможна,
 * в evidence-таблицы оно не пишется.
 */
export async function recordReferredSpendV2(
  database: SalesDatabase,
  event: ReferredSpendV2Event,
): Promise<SpendCommissionOutcome> {
  assertReferredSpendV2Shape(event);
  if (!isCommissionableSpendV2(event)) return "skipped";
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const referred = await client.query<{ partner_id: string }>(
      "SELECT partner_id FROM referred_users WHERE commerce_user_id = $1",
      [event.commerceUserId],
    );
    const directPartnerId = referred.rows[0]?.partner_id;
    if (!directPartnerId) {
      // Событие пришло раньше атрибуции юзера — буферизуем со всей lineage, reconcile проиграет
      // позже. Commerce-события immutable, поэтому конфликт — идемпотентный no-op без сверки полей.
      await client.query(`
        INSERT INTO pending_referral_usage_events_v2 (
          commerce_ref, commerce_event_id, commerce_user_id, provider_id, account_class,
          official_nano, charged_nano, paid_funded_nano, bonus_funded_nano, other_funded_nano,
          commission_eligible, release_generation, release_digest, snapshot_digest, occurred_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
        ON CONFLICT (commerce_event_id) DO NOTHING
      `, [
        pendingUsageV2Ref(event.commerceEventId), event.commerceEventId.toString(),
        event.commerceUserId, event.providerId, event.accountClass,
        event.officialNano.toString(), event.chargedNano.toString(),
        event.paidFundedNano.toString(), event.bonusFundedNano.toString(),
        event.otherFundedNano.toString(), event.commissionEligible,
        event.releaseGeneration.toString(), event.releaseDigest, event.snapshotDigest,
        event.occurredAt,
      ]);
      await client.query("COMMIT");
      return "buffered";
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO partner_usage_events_v2 (
        commerce_event_id, commerce_user_id, partner_id, provider_id, account_class,
        official_nano, charged_nano, paid_funded_nano, bonus_funded_nano, other_funded_nano,
        commission_eligible, release_generation, release_digest, snapshot_digest, occurred_at
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
      ON CONFLICT (commerce_event_id) DO NOTHING
      RETURNING id
    `, [
      event.commerceEventId.toString(), event.commerceUserId, directPartnerId,
      event.providerId, event.accountClass,
      event.officialNano.toString(), event.chargedNano.toString(),
      event.paidFundedNano.toString(), event.bonusFundedNano.toString(),
      event.otherFundedNano.toString(), event.commissionEligible,
      event.releaseGeneration.toString(), event.releaseDigest, event.snapshotDigest,
      event.occurredAt,
    ]);
    const usageEventId = inserted.rows[0]?.id;
    if (!usageEventId) {
      // Immutable evidence: повтор того же commerce_event_id — дубликат, второй записи не будет.
      await client.query("COMMIT");
      return "duplicate";
    }
    const chain = await loadCommissionChain(client, directPartnerId);
    for (const entry of computeCommissionChain(chain, event.paidFundedNano)) {
      await client.query(`
        INSERT INTO commission_entries_v2 (
          usage_event_id, partner_id, level, applied_bps, base_paid_funded_nano, amount_nano
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (usage_event_id, partner_id) DO NOTHING
      `, [
        usageEventId, entry.partnerId, entry.level, entry.appliedBps,
        event.paidFundedNano.toString(), entry.amountNano.toString(),
      ]);
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
 * Проигрывает буфер pending_referral_usage_events_v2 для юзеров, которые УЖЕ атрибутированы
 * (атрибуция пришла позже события). Каждая строка реплеится идемпотентно через
 * recordReferredSpendV2; recorded/duplicate/skipped удаляются из буфера, «buffered» (гонка —
 * юзер снова не атрибутирован) остаётся на следующий тик. Возвращает число обработанных строк.
 */
export async function reconcilePendingReferralUsageEventsV2(
  database: SalesDatabase,
  limit = 200,
): Promise<number> {
  const pending = await database.pool.query<{
    id: string;
    commerce_event_id: string;
    commerce_user_id: string;
    provider_id: string;
    account_class: string;
    official_nano: string;
    charged_nano: string;
    paid_funded_nano: string;
    bonus_funded_nano: string;
    other_funded_nano: string;
    commission_eligible: boolean;
    release_generation: string;
    release_digest: string;
    snapshot_digest: string;
    occurred_at: Date;
  }>(`
    SELECT pe.id, pe.commerce_event_id::text AS commerce_event_id, pe.commerce_user_id,
           pe.provider_id, pe.account_class,
           pe.official_nano::text AS official_nano, pe.charged_nano::text AS charged_nano,
           pe.paid_funded_nano::text AS paid_funded_nano,
           pe.bonus_funded_nano::text AS bonus_funded_nano,
           pe.other_funded_nano::text AS other_funded_nano,
           pe.commission_eligible, pe.release_generation::text AS release_generation,
           pe.release_digest, pe.snapshot_digest, pe.occurred_at
    FROM pending_referral_usage_events_v2 pe
    JOIN referred_users ru ON ru.commerce_user_id = pe.commerce_user_id
    ORDER BY pe.id
    LIMIT $1
  `, [limit]);

  let processed = 0;
  for (const row of pending.rows) {
    if (row.account_class !== "b2c") {
      throw new ReferredSpendV2ShapeError("buffered release-v2 usage event lost its B2C identity");
    }
    const outcome = await recordReferredSpendV2(database, {
      commerceEventId: BigInt(row.commerce_event_id),
      commerceUserId: row.commerce_user_id,
      providerId: row.provider_id,
      accountClass: row.account_class,
      officialNano: BigInt(row.official_nano),
      chargedNano: BigInt(row.charged_nano),
      paidFundedNano: BigInt(row.paid_funded_nano),
      bonusFundedNano: BigInt(row.bonus_funded_nano),
      otherFundedNano: BigInt(row.other_funded_nano),
      commissionEligible: row.commission_eligible,
      releaseGeneration: BigInt(row.release_generation),
      releaseDigest: row.release_digest,
      snapshotDigest: row.snapshot_digest,
      occurredAt: row.occurred_at,
    });
    if (outcome !== "buffered") {
      await database.pool.query(
        "DELETE FROM pending_referral_usage_events_v2 WHERE id = $1",
        [row.id],
      );
      processed += 1;
    }
  }
  return processed;
}
