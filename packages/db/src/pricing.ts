import { randomUUID } from "node:crypto";
import {
  B2C_PRICING_TIERS,
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

export function tierForSpend(spentNano: bigint): number {
  let tier = 0;
  for (let index = 1; index < B2C_PRICING_TIERS.length; index += 1) {
    if (spentNano >= B2C_PRICING_TIERS[index]!.spendThresholdNano) tier = index;
  }
  return tier;
}

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
           COALESCE(pm.spent_nano, 0) AS spent_nano
    FROM customer_profiles cp
    LEFT JOIN pricing_months pm
      ON pm.user_id = cp.user_id AND pm.month_start = cp.pricing_month_start
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
  const currentTier = row.current_tier ?? 0;
  const tier = B2C_PRICING_TIERS[currentTier]!;
  const nextTier = B2C_PRICING_TIERS[currentTier + 1];
  const spentNano = BigInt(row.spent_nano);
  return {
    customerType: "b2c",
    pricingMode: "progressive",
    monthStart: row.pricing_month_start.toISOString(),
    tier: tier.code,
    discountPercent: tier.discountPercent,
    multiplierBp: tier.multiplierBp,
    spentNano: spentNano.toString(),
    retentionSpendNano: tier.spendThresholdNano.toString(),
    nextTier: nextTier ? {
      tier: nextTier.code,
      discountPercent: nextTier.discountPercent,
      spendThresholdNano: nextTier.spendThresholdNano.toString(),
      remainingNano: (nextTier.spendThresholdNano > spentNano
        ? nextTier.spendThresholdNano - spentNano
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
    const result = await client.query<{ last_ledger_id: string }>(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id)
      VALUES ($1, $2)
      ON CONFLICT (engine_account_id) DO UPDATE SET updated_at = pricing_usage_cursors.updated_at
      RETURNING last_ledger_id
    `, [target.engineAccountId, target.userId]);
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
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const profileResult = await client.query<{ current_tier: number; pricing_month_start: Date }>(`
      SELECT current_tier, pricing_month_start FROM customer_profiles
      WHERE user_id = $1 AND customer_type = 'b2c' FOR UPDATE
    `, [target.userId]);
    const profile = profileResult.rows[0];
    if (!profile) {
      await client.query("ROLLBACK");
      return;
    }
    let lastLedgerId = 0n;
    for (const entry of entries) {
      const ledgerId = BigInt(entry.id);
      if (ledgerId > lastLedgerId) lastLedgerId = ledgerId;
      if (entry.kind !== "charge" || BigInt(entry.amount_nano) <= 0n) continue;
      const occurredAt = new Date(Number(BigInt(entry.ts)) * 1000);
      const inserted = await client.query<{ id: string }>(`
        INSERT INTO pricing_usage_events (
          id, user_id, engine_account_id, ledger_entry_id, amount_nano, occurred_at
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (engine_account_id, ledger_entry_id) DO NOTHING
        RETURNING id
      `, [randomUUID(), target.userId, target.engineAccountId, ledgerId.toString(), entry.amount_nano, occurredAt]);
      if (!inserted.rows[0]) continue;
      const monthStart = utcMonthStart(occurredAt);
      await client.query(`
        INSERT INTO pricing_months (
          id, user_id, month_start, opening_tier, highest_tier, spent_nano
        ) VALUES ($1, $2, $3, $4, $4, $5)
        ON CONFLICT (user_id, month_start) DO UPDATE
        SET spent_nano = pricing_months.spent_nano + EXCLUDED.spent_nano, updated_at = now()
      `, [randomUUID(), target.userId, monthStart, profile.current_tier, entry.amount_nano]);
    }
    await client.query(`
      UPDATE pricing_usage_cursors
      SET last_ledger_id = GREATEST(last_ledger_id, $3), updated_at = now()
      WHERE engine_account_id = $1 AND user_id = $2
    `, [target.engineAccountId, target.userId, lastLedgerId.toString()]);

    const currentMonth = await client.query<{ spent_nano: string }>(`
      SELECT spent_nano FROM pricing_months WHERE user_id = $1 AND month_start = $2
    `, [target.userId, profile.pricing_month_start]);
    const spent = BigInt(currentMonth.rows[0]?.spent_nano ?? "0");
    const earnedTier = tierForSpend(spent);
    if (earnedTier > profile.current_tier) {
      await applyTierChange(client, target, earnedTier, "b2c_upgrade");
      await client.query(`
        UPDATE pricing_months SET highest_tier = GREATEST(highest_tier, $3), updated_at = now()
        WHERE user_id = $1 AND month_start = $2
      `, [target.userId, profile.pricing_month_start, earnedTier]);
    }
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Close every elapsed UTC month, one locked profile/month at a time. */
export async function closeElapsedPricingMonths(database: Database, now = new Date()): Promise<number> {
  const currentMonth = utcMonthStart(now);
  let closed = 0;
  for (;;) {
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN");
      const result = await client.query<{
        user_id: string; current_tier: number; pricing_month_start: Date; engine_account_id: string | null;
      }>(`
        SELECT cp.user_id, cp.current_tier, cp.pricing_month_start, ea.engine_account_id
        FROM customer_profiles cp
        JOIN engine_accounts ea ON ea.user_id = cp.user_id
        WHERE cp.customer_type = 'b2c' AND cp.pricing_month_start < $1
        ORDER BY cp.pricing_month_start, cp.user_id
        FOR UPDATE OF cp, ea SKIP LOCKED
        LIMIT 1
      `, [currentMonth]);
      const row = result.rows[0];
      if (!row) {
        await client.query("COMMIT");
        return closed;
      }
      const month = await client.query<{ spent_nano: string }>(`
        INSERT INTO pricing_months (id, user_id, month_start, opening_tier, highest_tier)
        VALUES ($1, $2, $3, $4, $4)
        ON CONFLICT (user_id, month_start) DO UPDATE SET updated_at = pricing_months.updated_at
        RETURNING spent_nano
      `, [randomUUID(), row.user_id, row.pricing_month_start, row.current_tier]);
      const spent = BigInt(month.rows[0]?.spent_nano ?? "0");
      const retentionThreshold = B2C_PRICING_TIERS[row.current_tier]!.spendThresholdNano;
      const nextTier = spent >= retentionThreshold ? row.current_tier : Math.max(0, row.current_tier - 1);
      const nextMonthStart = addUtcMonth(row.pricing_month_start);
      await client.query(`
        UPDATE pricing_months SET closed_at = now(), updated_at = now()
        WHERE user_id = $1 AND month_start = $2
      `, [row.user_id, row.pricing_month_start]);
      await client.query(`
        UPDATE customer_profiles
        SET current_tier = $2, multiplier_bp = $3, pricing_month_start = $4, updated_at = now()
        WHERE user_id = $1
      `, [row.user_id, nextTier, B2C_PRICING_TIERS[nextTier]!.multiplierBp, nextMonthStart]);
      await client.query(`
        UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
      `, [row.user_id, B2C_PRICING_TIERS[nextTier]!.multiplierBp]);
      await client.query(`
        INSERT INTO pricing_months (id, user_id, month_start, opening_tier, highest_tier)
        VALUES ($1, $2, $3, $4, $4)
        ON CONFLICT (user_id, month_start) DO NOTHING
      `, [randomUUID(), row.user_id, nextMonthStart, nextTier]);
      if (nextTier !== row.current_tier && row.engine_account_id) {
        await enqueuePricingJob(client, {
          userId: row.user_id,
          engineAccountId: row.engine_account_id,
          multiplierBp: B2C_PRICING_TIERS[nextTier]!.multiplierBp,
          reason: "b2c_monthly_downgrade",
        });
      }
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
    UPDATE engine_pricing_jobs SET status = 'confirmed', confirmed_at = now(), locked_at = NULL,
      locked_by = NULL, last_error = NULL, updated_at = now()
    WHERE id = $1 AND status = 'processing' AND multiplier_bp = $2
  `, [job.id, job.multiplierBp]);
}

export async function retryPricingJob(database: Database, job: ClaimedPricingJob, error: string): Promise<void> {
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(job.attempts, 10)));
  await database.pool.query(`
    UPDATE engine_pricing_jobs SET status = 'retry', next_attempt_at = now() + ($3 * interval '1 second'),
      locked_at = NULL, locked_by = NULL, last_error = $2, updated_at = now()
    WHERE id = $1 AND status = 'processing' AND multiplier_bp = $4
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

async function applyTierChange(
  client: PoolClient,
  target: PricingSyncTarget,
  tier: number,
  reason: string,
): Promise<void> {
  const multiplierBp = B2C_PRICING_TIERS[tier]!.multiplierBp;
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
  const result = await client.query<{ id: string }>(`
    INSERT INTO engine_pricing_jobs (
      id, user_id, engine_account_id, multiplier_bp, reason
    ) VALUES ($1, $2, $3, $4, $5)
    ON CONFLICT (user_id) DO UPDATE SET
      engine_account_id = EXCLUDED.engine_account_id,
      multiplier_bp = EXCLUDED.multiplier_bp,
      reason = EXCLUDED.reason,
      status = 'pending', attempts = 0, next_attempt_at = now(),
      locked_at = NULL, locked_by = NULL, last_error = NULL,
      confirmed_at = NULL, updated_at = now()
    RETURNING id
  `, [randomUUID(), input.userId, input.engineAccountId, input.multiplierBp, input.reason]);
  return result.rows[0]!.id;
}

function addUtcMonth(value: Date): Date {
  return new Date(Date.UTC(value.getUTCFullYear(), value.getUTCMonth() + 1, 1));
}

interface PricingViewRow {
  customer_type: "b2c" | "b2b";
  current_tier: number | null;
  multiplier_bp: number;
  pricing_month_start: Date;
  spent_nano: string;
}
