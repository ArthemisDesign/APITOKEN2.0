import { randomUUID } from "node:crypto";
import {
  B2C_PRICING_TIERS,
  paymentProviderSchema,
  type EngineLedgerEntry,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";

export class InvalidBusinessInvitationError extends Error {}
export class BusinessCustomerNotFoundError extends Error {}

export interface PricingSyncTarget {
  userId: string;
  engineAccountId: string;
}

export interface ClaimedPricingJob {
  id: string;
  userId: string;
  engineAccountId: string;
  multiplierBp: number;
  attempts: number;
}

export function utcMonthStart(value = new Date()): Date {
  return new Date(Date.UTC(value.getUTCFullYear(), value.getUTCMonth(), 1));
}

// РЕАЛЬНЫМИ (комиссионируемыми) считаем ТОЛЬКО депозиты через платёжные провайдеры:
// engine ref = `${provider}:${providerPaymentId}` (см. payments.ts). Это whitelist, а не blacklist:
// welcome-бонус (`signup-bonus:`), промо (`promo:`), админ-кредит (`admin-credit:`), пустой или
// неизвестный ref — «бесплатное», комиссия по нему НЕ идёт. Так любой новый нереальный источник
// денег по умолчанию бесплатный, и мы никогда случайно не выплатим комиссию с подарка.
const REAL_MONEY_REF_PREFIXES: readonly string[] = paymentProviderSchema.options.map((p) => `${p}:`);

export function isFreeCreditRef(ref: string | null | undefined): boolean {
  if (typeof ref !== "string") return true;
  return !REAL_MONEY_REF_PREFIXES.some((prefix) => ref.startsWith(prefix));
}

/** Тир по НАКОПЛЕННОЙ сумме пополнений (`spendThresholdNano` = порог пополнения). 0 = none (<$100). */
export function tierForTopups(cumulativeNano: bigint): number {
  let tier = 0;
  for (let index = 1; index < B2C_PRICING_TIERS.length; index += 1) {
    if (cumulativeNano >= B2C_PRICING_TIERS[index]!.spendThresholdNano) tier = index;
  }
  return tier;
}

/** Тридцатидневное окно удержания в миллисекундах. */
const HOLD_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;
const PRICING_LEDGER_PAGE_SIZE = 1000;

export async function createBusinessInvite(database: Database, input: {
  email: string;
  tokenHash: string;
  multiplierBp: number;
  expiresAt: Date;
}): Promise<string> {
  const id = randomUUID();
  await database.pool.query(`
    INSERT INTO business_invites (id, email, token_hash, multiplier_bp, expires_at)
    VALUES ($1, $2, $3, $4, $5)
  `, [id, input.email.toLowerCase(), input.tokenHash, input.multiplierBp, input.expiresAt]);
  return id;
}

export async function lockBusinessInvite(
  client: PoolClient,
  input: { email: string; tokenHash: string },
): Promise<{ id: string; multiplierBp: number }> {
  const result = await client.query<{ id: string; multiplier_bp: number }>(`
    SELECT id, multiplier_bp
    FROM business_invites
    WHERE token_hash = $1 AND lower(email) = lower($2)
      AND consumed_at IS NULL AND expires_at > now()
    FOR UPDATE
  `, [input.tokenHash, input.email]);
  const invite = result.rows[0];
  if (!invite) throw new InvalidBusinessInvitationError("invalid, expired, or email-mismatched business invitation");
  return { id: invite.id, multiplierBp: invite.multiplier_bp };
}

export async function getPricingView(database: Database, userId: string): Promise<Record<string, unknown> | null> {
  const result = await database.pool.query<PricingViewRow>(`
    SELECT cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.pricing_month_start,
           cp.cumulative_topup_nano, cp.tier_window_start, cp.tier_window_spent_nano, cp.referral_floor_bps
    FROM customer_profiles cp
    WHERE cp.user_id = $1
  `, [userId]);
  const row = result.rows[0];
  if (!row) return null;
  const discountPercent = 100 - row.multiplier_bp / 100;
  if (row.customer_type === "b2b") {
    return {
      customerType: "b2b",
      pricingMode: "manual",
      discountPercent,
      multiplierBp: row.multiplier_bp,
    };
  }
  // Prepay-модель: поля формы сохранены, но переосмыслены —
  // spentNano = НАКОПЛЕНО пополнений; retentionSpendNano = сколько тратить за 30 дней (hold);
  // nextTier.remainingNano = сколько ещё ДОЛОЖИТЬ до следующего тира.
  const currentTier = row.current_tier ?? 0;
  const tier = B2C_PRICING_TIERS[currentTier]!;
  const nextTier = B2C_PRICING_TIERS[currentTier + 1];
  const cumulative = BigInt(row.cumulative_topup_nano);
  // Скидочный «пол» партнёра (реф-ссылка сейлза). Если задан — реальная цена = min(тир, 100−floor),
  // и клиент показывает фиксированную партнёрскую ставку вместо тир-лестницы.
  const floorBps = row.referral_floor_bps ?? 0;
  const effectiveBp = effectiveMultiplierBp(tier.multiplierBp, floorBps);
  return {
    customerType: "b2c",
    pricingMode: "progressive",
    monthStart: row.pricing_month_start.toISOString(),
    tier: tier.code,
    discountPercent: tier.discountPercent,
    multiplierBp: tier.multiplierBp,
    // Фиксированная партнёрская скидка (0 = нет): floor и итоговая эффективная ставка/скидка.
    referralFloorBps: floorBps,
    effectiveMultiplierBp: effectiveBp,
    effectiveDiscountPercent: 100 - effectiveBp / 100,
    spentNano: cumulative.toString(),
    retentionSpendNano: tier.holdNano.toString(),
    windowSpentNano: BigInt(row.tier_window_spent_nano).toString(),
    windowStart: row.tier_window_start ? row.tier_window_start.toISOString() : null,
    nextTier: nextTier ? {
      tier: nextTier.code,
      discountPercent: nextTier.discountPercent,
      spendThresholdNano: nextTier.spendThresholdNano.toString(),
      remainingNano: (nextTier.spendThresholdNano > cumulative
        ? nextTier.spendThresholdNano - cumulative
        : 0n).toString(),
      visibleOfficialUsageUsd: nextTier.visibleOfficialUsageUsd,
    } : null,
  };
}

export async function setBusinessPricing(database: Database, input: {
  userId: string;
  multiplierBp: number;
  actorId: string;
}): Promise<{ engineAccountId: string; jobId: string }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ engine_account_id: string }>(`
      SELECT ea.engine_account_id
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1 AND cp.customer_type = 'b2b'
        AND ea.engine_account_id IS NOT NULL
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    if (!row) throw new BusinessCustomerNotFoundError("business customer not found");
    await client.query(`
      UPDATE customer_profiles SET multiplier_bp = $2, updated_at = now() WHERE user_id = $1;
    `, [input.userId, input.multiplierBp]);
    await client.query(`
      UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
    `, [input.userId, input.multiplierBp]);
    const jobId = await enqueuePricingJob(client, {
      userId: input.userId,
      engineAccountId: row.engine_account_id,
      multiplierBp: input.multiplierBp,
      reason: "b2b_manual",
    });
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing.b2b_changed', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({ multiplierBp: input.multiplierBp })]);
    await client.query("COMMIT");
    return { engineAccountId: row.engine_account_id, jobId };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Устанавливает «пол» скидки от сейлза для реферала ПАРТНЁРА. Клиент ОСТАЁТСЯ b2c и идёт по
 * обычным тир-правилам (тир, бонус, промо — как у всех); floor лишь гарантирует, что цена не хуже
 * скидки сейлза: эффективный mult = min(тир-mult, 10000 - floorBps). floorBps=0 снимает пол
 * (возврат к чистому тиру). Разграничение b2c/b2b сохранено: для business-b2b пол не применяется.
 * Мультипликатор доставляется в движок через durable engine_pricing_jobs. Идемпотентно.
 * Вызывается ТОЛЬКО для партнёрских реф-кодов (обычная сайтовая рефка сюда не идёт).
 */
export async function setReferralFloor(database: Database, input: {
  userId: string;
  floorBps: number; // 0..9500 (скидка ≤ 95%); 0 = снять пол
  actorId: string;
}): Promise<{ applied: boolean; multiplierBp: number | null }> {
  if (!Number.isInteger(input.floorBps) || input.floorBps < 0 || input.floorBps > 9500) {
    throw new RangeError("referral floor must be an integer between 0 and 9500 bps (≤95%)");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      engine_account_id: string | null; customer_type: "b2c" | "b2b";
      current_tier: number | null; multiplier_bp: number; referral_floor_bps: number;
    }>(`
      SELECT ea.engine_account_id, cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.referral_floor_bps
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    // Пол применяется только к обычным b2c-аккаунтам; business-b2b не трогаем (своя прайс-логика).
    if (!row || row.customer_type !== "b2c" || row.current_tier === null) {
      await client.query("ROLLBACK");
      return { applied: false, multiplierBp: null };
    }
    const tierMult = B2C_PRICING_TIERS[row.current_tier]!.multiplierBp;
    // «Пол» скидки — МОНОТОНЕН: floorBps>0 только ПОДНИМАЕТ (GREATEST). Колонка одна, но пишут в неё
    // три независимых источника (промо, партнёрская ссылка, sales-фид) — при абсолютной записи они
    // затирали друг друга и клиент молча терял лучшую скидку. Теперь берём максимум (лучшее клиенту).
    // floorBps===0 — единственный путь ЯВНОГО сброса пола (напр. отзыв), обходит GREATEST.
    const effectiveFloor = input.floorBps === 0 ? 0 : Math.max(row.referral_floor_bps, input.floorBps);
    const multiplierBp = effectiveMultiplierBp(tierMult, effectiveFloor);
    if (row.referral_floor_bps === effectiveFloor && row.multiplier_bp === multiplierBp) {
      await client.query("ROLLBACK");
      return { applied: false, multiplierBp };
    }
    await client.query(`
      UPDATE customer_profiles
      SET referral_floor_bps = $2, multiplier_bp = $3, updated_at = now()
      WHERE user_id = $1
    `, [input.userId, effectiveFloor, multiplierBp]);
    await client.query(`
      UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
    `, [input.userId, multiplierBp]);
    if (row.engine_account_id) {
      await enqueuePricingJob(client, {
        userId: input.userId,
        engineAccountId: row.engine_account_id,
        multiplierBp,
        reason: "referral_floor",
      });
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('system', $1, 'pricing.referral_floor', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({ requestedFloorBps: input.floorBps, effectiveFloorBps: effectiveFloor, multiplierBp })]);
    await client.query("COMMIT");
    return { applied: true, multiplierBp };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function listPricingSyncTargets(database: Database): Promise<PricingSyncTarget[]> {
  const result = await database.pool.query<{ user_id: string; engine_account_id: string }>(`
    SELECT cp.user_id, ea.engine_account_id
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    JOIN users u ON u.id = cp.user_id
    WHERE cp.customer_type = 'b2c' AND ea.status = 'active'
      AND ea.engine_account_id IS NOT NULL AND u.status = 'active'
    ORDER BY cp.user_id
  `);
  return result.rows.map((row) => ({ userId: row.user_id, engineAccountId: row.engine_account_id }));
}

export async function getPricingUsageCursor(
  database: Database,
  target: PricingSyncTarget,
): Promise<bigint> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(`
      DELETE FROM pricing_usage_cursors WHERE user_id = $1 AND engine_account_id <> $2
    `, [target.userId, target.engineAccountId]);
    // Invalidate the completion marker before network I/O. Only a terminal short page restores it;
    // a thrown/failed sync therefore cannot authorize window closure with a previous cycle's marker.
    const result = await client.query<{ last_ledger_id: string }>(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id, updated_at)
      VALUES ($1, $2, '-infinity')
      ON CONFLICT (engine_account_id) DO UPDATE SET updated_at = '-infinity'
      RETURNING last_ledger_id
    `, [target.engineAccountId, target.userId]);
    // Reconcile durable credit accrual markers on every pricing poll. This catches a missed
    // post-credit call and reverses markers whose payment has since been refunded/disputed.
    await reconcileTopupTier(client, target, "b2c_topup_reconcile");
    await client.query("COMMIT");
    return BigInt(result.rows[0]?.last_ledger_id ?? "0");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function applyPricingLedgerPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
): Promise<void> {
  if (entries.length === 0) return;
  // Free-first требует ХРОНОЛОГИЧЕСКОГО порядка: топап должен фондировать последующие charge.
  // Леджер-id движка монотонны по времени создания, поэтому сортируем страницу по id по возрастанию —
  // иначе charge, пришедший в массиве раньше своего фондирующего топапа, завысил бы real_funded (и,
  // как следствие, реф-комиссию). Это же делает продвижение курсора детерминированным.
  const ordered = [...entries].sort((a, b) => {
    const da = BigInt(a.id);
    const db = BigInt(b.id);
    return da < db ? -1 : da > db ? 1 : 0;
  });
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const profileResult = await client.query<{ current_tier: number; pricing_month_start: Date; free_balance_nano: string }>(`
      SELECT current_tier, pricing_month_start, free_balance_nano::text AS free_balance_nano FROM customer_profiles
      WHERE user_id = $1 AND customer_type = 'b2c' FOR UPDATE
    `, [target.userId]);
    const profile = profileResult.rows[0];
    if (!profile) {
      await client.query("ROLLBACK");
      return;
    }
    // Курсор на старте: применяем эффекты (free-баланс, real_funded) ТОЛЬКО к записям выше него —
    // это делает применение страницы идемпотентным к повторной подаче тех же записей (free-топапы
    // не имеют ON CONFLICT, как у charge). customer_profiles залочена → обработка юзера сериализована.
    const cursorRow = await client.query<{ last_ledger_id: string }>(
      "SELECT last_ledger_id::text AS last_ledger_id FROM pricing_usage_cursors WHERE engine_account_id = $1 AND user_id = $2",
      [target.engineAccountId, target.userId],
    );
    const startCursor = BigInt(cursorRow.rows[0]?.last_ledger_id ?? "0");
    let freeBalance = BigInt(profile.free_balance_nano);
    let freeBalanceChanged = false;
    let lastLedgerId = 0n;
    let insertedCharge = false;
    for (const entry of ordered) {
      const ledgerId = BigInt(entry.id);
      if (ledgerId > lastLedgerId) lastLedgerId = ledgerId;
      if (ledgerId <= startCursor) continue; // уже обработано ранее — не двоим эффекты
      const amount = BigInt(entry.amount_nano);
      // Бесплатные кредиты (welcome-бонус/промо) пополняют бесплатный баланс.
      if (entry.kind === "topup" && amount > 0n && isFreeCreditRef(entry.ref)) {
        freeBalance += amount;
        freeBalanceChanged = true;
        continue;
      }
      // Только положительные charge создают real_funded/комиссию. Отрицательные `adjust`
      // (рефанд/чарджбэк/админ-клобэк) здесь НЕ откатывают уже начисленную комиссию — это
      // осознанный остаток (полный клобэк требует негативного real_funded по всей цепочке фида
      // и завязан на engine-компенсацию рефанда). Игнорирование безопасно по направлению: оно
      // может только НЕ доплатить, но не переплачивает.
      if (entry.kind !== "charge" || amount <= 0n) continue;
      const occurredAt = new Date(Number(BigInt(entry.ts)) * 1000);
      // Бесплатное тратится первым: real_funded — часть, покрытая реальными деньгами.
      const fromFree = amount < freeBalance ? amount : freeBalance;
      const realFunded = amount - fromFree;
      const inserted = await client.query<{ id: string }>(`
        INSERT INTO pricing_usage_events (
          id, user_id, engine_account_id, ledger_entry_id, amount_nano, real_funded_nano, occurred_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (engine_account_id, ledger_entry_id) DO NOTHING
        RETURNING id
      `, [randomUUID(), target.userId, target.engineAccountId, ledgerId.toString(), entry.amount_nano, realFunded.toString(), occurredAt]);
      if (!inserted.rows[0]) continue; // уже обработано — бесплатный баланс не трогаем повторно
      if (fromFree > 0n) { freeBalance -= fromFree; freeBalanceChanged = true; }
      insertedCharge = true;
      const monthStart = utcMonthStart(occurredAt);
      await client.query(`
        INSERT INTO pricing_months (
          id, user_id, month_start, opening_tier, highest_tier, spent_nano
        ) VALUES ($1, $2, $3, $4, $4, $5)
        ON CONFLICT (user_id, month_start) DO UPDATE
        SET spent_nano = pricing_months.spent_nano + EXCLUDED.spent_nano, updated_at = now()
      `, [randomUUID(), target.userId, monthStart, profile.current_tier, entry.amount_nano]);
    }
    if (freeBalanceChanged) {
      await client.query(
        "UPDATE customer_profiles SET free_balance_nano = $2, updated_at = now() WHERE user_id = $1",
        [target.userId, freeBalance.toString()],
      );
    }
    const reachedStablePageEnd = entries.length < PRICING_LEDGER_PAGE_SIZE;
    await client.query(`
      UPDATE pricing_usage_cursors
      SET last_ledger_id = GREATEST(last_ledger_id, $3),
          updated_at = CASE WHEN $4 THEN now() ELSE updated_at END
      WHERE engine_account_id = $1 AND user_id = $2
    `, [target.engineAccountId, target.userId, lastLedgerId.toString(), reachedStablePageEnd]);

    // Prepay: расход НЕ поднимает тир (тир — за пополнения). The cached counter is rebuilt
    // from immutable events in the exact current [window_start, window_end) interval so late
    // ingestion cannot move a charge across retention windows.
    if (insertedCharge) await refreshCurrentTierWindowSpend(client, target.userId);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Prepay-тир: применяет ещё не учтённые confirmed-кредиты через durable accrual markers.
 * Повторный вызов идемпотентен, пропущенный вызов догоняется следующим reconcile, а marker
 * refunded/disputed-платежа удаляется с компенсирующим уменьшением cumulative.
 */
export async function applyTopupTier(database: Database, input: {
  engineAccountId: string;
  amountNano: bigint;
}): Promise<void> {
  if (input.amountNano <= 0n) throw new RangeError("top-up amount must be positive");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await reconcileTopupTier(client, {
      engineAccountId: input.engineAccountId,
    }, "b2c_topup");
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Закрытие 30-дневных окон удержания: у кого окно истекло — если за окно потрачено ≥ hold(tier),
 * окно продлевается; иначе откат на −1 тир, накопление сбрасывается к порогу нового тира, окно новое.
 */
export async function closeElapsedTierWindows(database: Database, now = new Date()): Promise<number> {
  const windowDeadline = new Date(now.getTime() - HOLD_WINDOW_MS);
  let closed = 0;
  for (;;) {
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN");
      const result = await client.query<{
        user_id: string; current_tier: number; tier_window_start: Date; engine_account_id: string;
      }>(`
        SELECT cp.user_id, cp.current_tier, cp.tier_window_start, ea.engine_account_id
        FROM customer_profiles cp
        JOIN engine_accounts ea ON ea.user_id = cp.user_id
        JOIN pricing_usage_cursors puc
          ON puc.user_id = cp.user_id AND puc.engine_account_id = ea.engine_account_id
        WHERE cp.customer_type = 'b2c' AND cp.current_tier > 0
          AND ea.engine_account_id IS NOT NULL
          AND cp.tier_window_start IS NOT NULL AND cp.tier_window_start <= $1
          -- A page shorter than the engine limit marks a completed ledger scan. If the current
          -- scan failed, updated_at is not advanced and this window is deferred rather than closed
          -- from incomplete usage.
          AND puc.updated_at >= cp.tier_window_start + interval '30 days'
        ORDER BY cp.tier_window_start, cp.user_id
        FOR UPDATE OF cp, ea SKIP LOCKED
        LIMIT 1
      `, [windowDeadline]);
      const row = result.rows[0];
      if (!row) {
        await client.query("COMMIT");
        return closed;
      }
      const windowEnd = new Date(row.tier_window_start.getTime() + HOLD_WINDOW_MS);
      const spentResult = await client.query<{ spent_nano: string }>(`
        SELECT COALESCE(SUM(amount_nano), 0)::text AS spent_nano
        FROM pricing_usage_events
        WHERE user_id = $1 AND engine_account_id = $2
          AND occurred_at >= $3 AND occurred_at < $4
      `, [row.user_id, row.engine_account_id, row.tier_window_start, windowEnd]);
      const windowSpent = BigInt(spentResult.rows[0]?.spent_nano ?? "0");
      const held = windowSpent >= B2C_PRICING_TIERS[row.current_tier]!.holdNano;
      if (held) {
        await client.query(`
          UPDATE customer_profiles SET tier_window_start = $2, tier_window_spent_nano = 0, updated_at = now()
          WHERE user_id = $1
        `, [row.user_id, windowEnd]);
      } else {
        // Не удержал — откат на −1 тир; накопление к порогу нового тира; новое окно (или none → без окна).
        const nextTier = Math.max(0, row.current_tier - 1);
        const newCumulative = B2C_PRICING_TIERS[nextTier]!.spendThresholdNano;
        await applyTierChange(client, { userId: row.user_id, engineAccountId: row.engine_account_id }, nextTier, "b2c_window_downgrade");
        await client.query(`
          UPDATE customer_profiles
          SET cumulative_topup_nano = $2, tier_window_start = $3, tier_window_spent_nano = 0, updated_at = now()
          WHERE user_id = $1
        `, [row.user_id, newCumulative.toString(), nextTier > 0 ? windowEnd : null]);
      }
      // Carry already-ingested post-cutoff charges into the exact next window instead of losing them.
      await refreshCurrentTierWindowSpend(client, row.user_id);
      // AUDIT-TODO(C19): persist an explicit engine cutoff watermark; cursor freshness is the
      // safest localized guard available until the Control API exposes a stable ledger watermark.
      await client.query("COMMIT");
      closed += 1;
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }
}

export async function claimNextPricingJob(
  database: Database,
  workerId: string,
): Promise<ClaimedPricingJob | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // Lease recovery is part of normal claiming, not a startup-only maintenance step. A failed
    // retryPricingJob write therefore delays a job by at most one lease interval instead of
    // stranding it in processing until process restart.
    await client.query(`
      UPDATE engine_pricing_jobs
      SET status = 'retry', locked_at = NULL, locked_by = NULL, next_attempt_at = now(),
          last_error = COALESCE(last_error, 'recovered expired pricing lease'), updated_at = now()
      WHERE status = 'processing'
        AND (locked_at IS NULL OR locked_at < now() - interval '5 minutes')
    `);
    const result = await client.query<{
      id: string; user_id: string; engine_account_id: string; multiplier_bp: number; attempts: number;
    }>(`
      SELECT id, user_id, engine_account_id, multiplier_bp, attempts
      FROM engine_pricing_jobs
      WHERE status IN ('pending', 'retry') AND next_attempt_at <= now()
      ORDER BY next_attempt_at, created_at
      FOR UPDATE SKIP LOCKED LIMIT 1
    `);
    const row = result.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }
    await client.query(`
      UPDATE engine_pricing_jobs SET status = 'processing', locked_at = now(), locked_by = $2,
        attempts = attempts + 1, updated_at = now() WHERE id = $1
    `, [row.id, workerId]);
    await client.query("COMMIT");
    return {
      id: row.id,
      userId: row.user_id,
      engineAccountId: row.engine_account_id,
      multiplierBp: row.multiplier_bp,
      attempts: row.attempts + 1,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function confirmPricingJob(database: Database, job: ClaimedPricingJob): Promise<void> {
  await database.pool.query(`
    UPDATE engine_pricing_jobs job
    SET engine_account_id = COALESCE(ea.engine_account_id, job.engine_account_id),
        multiplier_bp = cp.multiplier_bp,
        reason = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.reason
          ELSE 'superseded_after_processing'
        END,
        status = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN 'confirmed'::pricing_job_status
          ELSE 'pending'::pricing_job_status
        END,
        attempts = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.attempts ELSE 0 END,
        next_attempt_at = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.next_attempt_at ELSE now() END,
        confirmed_at = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN now() ELSE NULL END,
        locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
    FROM customer_profiles cp
    LEFT JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE job.id = $1 AND job.status = 'processing' AND job.multiplier_bp = $2
      AND cp.user_id = job.user_id
  `, [job.id, job.multiplierBp]);
}

export async function retryPricingJob(database: Database, job: ClaimedPricingJob, error: string): Promise<void> {
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(job.attempts, 10)));
  await database.pool.query(`
    UPDATE engine_pricing_jobs job
    SET engine_account_id = COALESCE(ea.engine_account_id, job.engine_account_id),
        multiplier_bp = cp.multiplier_bp,
        reason = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.reason
          ELSE 'superseded_after_processing'
        END,
        status = 'retry',
        attempts = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN job.attempts ELSE 0 END,
        next_attempt_at = CASE
          WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN now() + ($3 * interval '1 second')
          ELSE now()
        END,
        locked_at = NULL, locked_by = NULL,
        last_error = CASE WHEN cp.multiplier_bp = job.multiplier_bp AND COALESCE(ea.engine_account_id, job.engine_account_id) = job.engine_account_id THEN $2 ELSE NULL END,
        updated_at = now()
    FROM customer_profiles cp
    LEFT JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE job.id = $1 AND job.status = 'processing' AND job.multiplier_bp = $4
      AND cp.user_id = job.user_id
  `, [job.id, error.slice(0, 2000), delaySeconds, job.multiplierBp]);
}

export async function recoverStalePricingJobs(database: Database): Promise<number> {
  const result = await database.pool.query(`
    UPDATE engine_pricing_jobs SET status = 'retry', locked_at = NULL, locked_by = NULL,
      next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}

/** Эффективный mult = лучшее из тира и «пола» скидки сейлза (меньший mult = большая скидка). */
function effectiveMultiplierBp(tierMultiplierBp: number, referralFloorBps: number): number {
  return Math.min(tierMultiplierBp, 10_000 - referralFloorBps);
}

async function applyTierChange(
  client: PoolClient,
  target: PricingSyncTarget,
  tier: number,
  reason: string,
): Promise<void> {
  // Реф-скидка сейлза — «пол»: тир даёт свою скидку, но цена не хуже скидки сейлза. floor=0 → тир как есть.
  const floorResult = await client.query<{ referral_floor_bps: number }>(
    "SELECT referral_floor_bps FROM customer_profiles WHERE user_id = $1",
    [target.userId],
  );
  const floorBps = floorResult.rows[0]?.referral_floor_bps ?? 0;
  const multiplierBp = effectiveMultiplierBp(B2C_PRICING_TIERS[tier]!.multiplierBp, floorBps);
  await client.query(`
    UPDATE customer_profiles SET current_tier = $2, multiplier_bp = $3, updated_at = now()
    WHERE user_id = $1 AND customer_type = 'b2c'
  `, [target.userId, tier, multiplierBp]);
  await client.query(`UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1`, [
    target.userId, multiplierBp,
  ]);
  await enqueuePricingJob(client, {
    userId: target.userId,
    engineAccountId: target.engineAccountId,
    multiplierBp,
    reason,
  });
}

async function enqueuePricingJob(client: PoolClient, input: {
  userId: string; engineAccountId: string; multiplierBp: number; reason: string;
}): Promise<string> {
  const existing = await client.query<{ id: string; status: "pending" | "processing" | "retry" | "confirmed" }>(`
    SELECT id, status FROM engine_pricing_jobs WHERE user_id = $1 FOR UPDATE
  `, [input.userId]);
  const row = existing.rows[0];
  if (row?.status === "processing") {
    // Do not revoke an active lease or let a newer generation run concurrently. The desired
    // multiplier is already durable on customer_profiles; confirmPricingJob requeues this row
    // after the in-flight engine request completes if that desired value changed.
    return row.id;
  }
  if (row) {
    const updated = await client.query<{ id: string }>(`
      UPDATE engine_pricing_jobs SET
        engine_account_id = $2, multiplier_bp = $3, reason = $4,
        status = 'pending', attempts = 0, next_attempt_at = now(),
        locked_at = NULL, locked_by = NULL, last_error = NULL,
        confirmed_at = NULL, updated_at = now()
      WHERE id = $1
      RETURNING id
    `, [row.id, input.engineAccountId, input.multiplierBp, input.reason]);
    return updated.rows[0]!.id;
  }
  const id = randomUUID();
  await client.query(`
    INSERT INTO engine_pricing_jobs (
      id, user_id, engine_account_id, multiplier_bp, reason
    ) VALUES ($1, $2, $3, $4, $5)
  `, [id, input.userId, input.engineAccountId, input.multiplierBp, input.reason]);
  return id;
}

async function reconcileTopupTier(
  client: PoolClient,
  target: { engineAccountId: string; userId?: string },
  reason: string,
): Promise<void> {
  const profileResult = await client.query<{
    user_id: string; current_tier: number; cumulative_topup_nano: string;
  }>(`
    SELECT cp.user_id, cp.current_tier, cp.cumulative_topup_nano
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE ea.engine_account_id = $1 AND cp.customer_type = 'b2c'
      AND ($2::uuid IS NULL OR cp.user_id = $2::uuid)
    FOR UPDATE OF cp
  `, [target.engineAccountId, target.userId ?? null]);
  const profile = profileResult.rows[0];
  if (!profile) return;

  // AUDIT-TODO(C21): run pnpm db:generate + migrate for pricing_credit_accruals.
  const appliedResult = await client.query<{ amount_nano: string }>(`
    WITH eligible AS (
      SELECT ec.id AS credit_id
      FROM engine_credits ec
      JOIN payments p ON p.id = ec.payment_id
      LEFT JOIN pricing_credit_accruals pca ON pca.credit_id = ec.id
      WHERE ec.engine_account_id = $1 AND ec.status = 'confirmed'
        AND p.user_id = $2 AND p.status = 'paid' AND pca.credit_id IS NULL
    ), inserted AS (
      INSERT INTO pricing_credit_accruals (credit_id)
      SELECT credit_id FROM eligible
      ON CONFLICT (credit_id) DO NOTHING
      RETURNING credit_id
    )
    SELECT COALESCE(SUM(ec.amount_nano), 0)::text AS amount_nano
    FROM inserted i
    JOIN engine_credits ec ON ec.id = i.credit_id
  `, [target.engineAccountId, profile.user_id]);
  const reversedResult = await client.query<{ amount_nano: string }>(`
    WITH removed AS (
      DELETE FROM pricing_credit_accruals pca
      USING engine_credits ec, payments p
      WHERE pca.credit_id = ec.id AND ec.payment_id = p.id
        AND ec.engine_account_id = $1 AND p.user_id = $2
        AND p.status IN ('refunded', 'disputed')
      RETURNING ec.amount_nano
    )
    SELECT COALESCE(SUM(amount_nano), 0)::text AS amount_nano FROM removed
  `, [target.engineAccountId, profile.user_id]);

  const applied = BigInt(appliedResult.rows[0]?.amount_nano ?? "0");
  const reversed = BigInt(reversedResult.rows[0]?.amount_nano ?? "0");
  if (applied === 0n && reversed === 0n) return;
  const currentCumulative = BigInt(profile.cumulative_topup_nano);
  const cumulative = currentCumulative + applied > reversed
    ? currentCumulative + applied - reversed
    : 0n;
  const currentTier = profile.current_tier ?? 0;
  const newTier = tierForTopups(cumulative);
  await client.query(`
    UPDATE customer_profiles SET cumulative_topup_nano = $2, updated_at = now() WHERE user_id = $1
  `, [profile.user_id, cumulative.toString()]);
  if (newTier !== currentTier) {
    await applyTierChange(client, {
      userId: profile.user_id,
      engineAccountId: target.engineAccountId,
    }, newTier, reversed > 0n ? "b2c_refund_reversal" : reason);
    await client.query(`
      UPDATE customer_profiles
      SET tier_window_start = CASE WHEN $2 > 0 THEN now() ELSE NULL END,
          tier_window_spent_nano = 0, updated_at = now()
      WHERE user_id = $1
    `, [profile.user_id, newTier]);
    await refreshCurrentTierWindowSpend(client, profile.user_id);
  }
}

async function refreshCurrentTierWindowSpend(client: PoolClient, userId: string): Promise<void> {
  await client.query(`
    UPDATE customer_profiles cp
    SET tier_window_spent_nano = CASE
          WHEN cp.tier_window_start IS NULL THEN 0
          ELSE COALESCE((
            SELECT SUM(pue.amount_nano)
            FROM pricing_usage_events pue
            WHERE pue.user_id = cp.user_id AND pue.engine_account_id = ea.engine_account_id
              AND pue.occurred_at >= cp.tier_window_start
              AND pue.occurred_at < cp.tier_window_start + interval '30 days'
          ), 0)
        END,
        updated_at = now()
    FROM engine_accounts ea
    WHERE cp.user_id = $1 AND cp.customer_type = 'b2c' AND ea.user_id = cp.user_id
  `, [userId]);
}

interface PricingViewRow {
  customer_type: "b2c" | "b2b";
  current_tier: number | null;
  multiplier_bp: number;
  pricing_month_start: Date;
  cumulative_topup_nano: string;
  tier_window_start: Date | null;
  tier_window_spent_nano: string;
  referral_floor_bps: number | null;
}
