import { randomUUID } from "node:crypto";
import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  applyPricingLedgerPage,
  claimNextPricingJob,
  confirmPricingJob,
  getPricingUsageCursor,
  listPricingSyncTargets,
  recoverStalePricingJobs,
  retryPricingJob,
  tierForTopups,
  utcMonthStart,
  type Database,
  type PricingSyncTarget,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

const HOLD_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;
const REFUND_REVERSAL_PROVIDER = "pricing-worker";

// Canonical top-up thresholds and multipliers. Advancement remains top-up-based; usage only retains a tier.
const B2C_TIER_RULES = [
  { thresholdNano: 0n, holdNano: 0n, multiplierBp: 4000 },
  { thresholdNano: 100_000_000_000n, holdNano: 50_000_000_000n, multiplierBp: 3500 },
  { thresholdNano: 250_000_000_000n, holdNano: 125_000_000_000n, multiplierBp: 3000 },
  { thresholdNano: 500_000_000_000n, holdNano: 250_000_000_000n, multiplierBp: 2500 },
  { thresholdNano: 1_000_000_000_000n, holdNano: 500_000_000_000n, multiplierBp: 2000 },
] as const;

@Injectable()
export class PricingWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(PricingWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    const recovered = await recoverStalePricingJobs(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale pricing jobs`);
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("PRICING_POLL_MS", { infer: true });
    this.logger.log(`pricing worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        // C68: recovery is part of normal polling, so a failed retry-state write cannot strand a
        // processing lease until process restart.
        const recovered = await recoverStalePricingJobs(this.database);
        if (recovered > 0) this.logger.warn(`recovered ${recovered} stale pricing jobs`);

        const reversed = await this.reverseRefundedTopups();
        if (reversed > 0) this.logger.log(`reversed ${reversed} refunded top-up contributions`);

        const now = new Date();
        const targets = await listPricingSyncTargets(this.database);
        const syncedUserIds: string[] = [];
        for (const target of targets) {
          if (this.stopped) break;
          try {
            await this.syncTarget(target);
            syncedUserIds.push(target.userId);
          } catch (error) {
            this.logger.error(`pricing usage sync failed for ${target.userId}: ${message(error)}`);
          }
        }

        // C20: the denormalized counter is rebuilt from immutable, deduplicated events using each
        // event's own timestamp instead of trusting the page in which the event happened to arrive.
        await this.reconcileTierWindowUsage(syncedUserIds, now);

        if (
          syncedUserIds.length > 0 &&
          afterMonthCloseGrace(now, this.config.get("PRICING_CLOSE_GRACE_MS", { infer: true }))
        ) {
          // C19: only users whose ledger sync completed in this cycle are eligible for closure.
          // AUDIT-TODO(C19): run pnpm db:generate + migrate after adding a durable per-account
          // engine cutoff watermark; require it to cover the exact window end before closure.
          const closed = await this.closeElapsedTierWindows(syncedUserIds, now);
          if (closed > 0) {
            await this.reconcileTierWindowUsage(syncedUserIds, now);
            this.logger.log(`closed ${closed} elapsed pricing windows`);
          }
        }
        await this.flushPricingJobs();
      } catch (error) {
        this.logger.error(message(error));
      }
      await this.sleep(pollMs);
    }
  }

  private async syncTarget(target: PricingSyncTarget): Promise<void> {
    let cursor = await getPricingUsageCursor(this.database, target);
    // If the previous acknowledgement failed after the commerce transaction committed, replay the
    // durable cursor before fetching. Otherwise an idle ledger would never give retention another
    // opportunity to observe that already-applied page.
    await this.engine.acknowledgeLedger(target.engineAccountId, cursor);
    for (;;) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) return;
      await applyPricingLedgerPage(this.database, target, entries);
      cursor = BigInt(entries.at(-1)!.id);
      await this.engine.acknowledgeLedger(target.engineAccountId, cursor);
      if (entries.length < 1000) return;
    }
  }

  private async reconcileTierWindowUsage(userIds: readonly string[], now: Date): Promise<void> {
    if (userIds.length === 0) return;
    await this.database.pool.query(`
      UPDATE customer_profiles cp
      SET tier_window_spent_nano = COALESCE((
            SELECT SUM(event.amount_nano)
            FROM pricing_usage_events event
            WHERE event.user_id = cp.user_id
              AND event.occurred_at >= cp.tier_window_start
              AND event.occurred_at < LEAST(
                $2::timestamptz,
                cp.tier_window_start + interval '30 days'
              )
          ), 0)::bigint,
          updated_at = now()
      WHERE cp.customer_type = 'b2c'
        AND cp.current_tier > 0
        AND cp.tier_window_start IS NOT NULL
        AND cp.user_id = ANY($1::uuid[])
    `, [userIds, now]);
  }

  private async closeElapsedTierWindows(userIds: readonly string[], now: Date): Promise<number> {
    let closed = 0;
    for (;;) {
      const client = await this.database.pool.connect();
      try {
        await client.query("BEGIN");
        const result = await client.query<{
          user_id: string;
          current_tier: number;
          cumulative_topup_nano: string;
          tier_window_start: Date;
          engine_account_id: string;
        }>(`
          SELECT cp.user_id, cp.current_tier, cp.cumulative_topup_nano,
                 cp.tier_window_start, ea.engine_account_id
          FROM customer_profiles cp
          JOIN engine_accounts ea ON ea.user_id = cp.user_id
          WHERE cp.customer_type = 'b2c'
            AND cp.current_tier > 0
            AND cp.tier_window_start IS NOT NULL
            AND cp.tier_window_start <= $1::timestamptz - interval '30 days'
            AND cp.user_id = ANY($2::uuid[])
            AND ea.engine_account_id IS NOT NULL
          ORDER BY cp.tier_window_start, cp.user_id
          FOR UPDATE OF cp, ea SKIP LOCKED
          LIMIT 1
        `, [now, userIds]);
        const row = result.rows[0];
        if (!row) {
          await client.query("COMMIT");
          return closed;
        }

        const currentRule = B2C_TIER_RULES[row.current_tier];
        if (!currentRule) throw new Error(`invalid B2C pricing tier ${row.current_tier}`);
        const windowEnd = new Date(row.tier_window_start.getTime() + HOLD_WINDOW_MS);
        const usage = await client.query<{ spent_nano: string }>(`
          SELECT COALESCE(SUM(amount_nano), 0)::text AS spent_nano
          FROM pricing_usage_events
          WHERE user_id = $1
            AND occurred_at >= $2
            AND occurred_at < $3
        `, [row.user_id, row.tier_window_start, windowEnd]);
        const windowSpentNano = BigInt(usage.rows[0]?.spent_nano ?? "0");
        const held = windowSpentNano >= currentRule.holdNano;
        const nextTier = held ? row.current_tier : Math.max(0, row.current_tier - 1);
        const nextRule = B2C_TIER_RULES[nextTier]!;
        const cumulativeTopupNano = held
          ? BigInt(row.cumulative_topup_nano)
          : nextRule.thresholdNano;

        await client.query(`
          UPDATE customer_profiles
          SET current_tier = $2,
              multiplier_bp = $3,
              cumulative_topup_nano = $4,
              tier_window_start = $5,
              tier_window_spent_nano = 0,
              updated_at = now()
          WHERE user_id = $1 AND customer_type = 'b2c'
        `, [
          row.user_id,
          nextTier,
          nextRule.multiplierBp,
          cumulativeTopupNano.toString(),
          nextTier > 0 ? windowEnd : null,
        ]);

        if (nextTier !== row.current_tier) {
          await client.query(`
            UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
          `, [row.user_id, nextRule.multiplierBp]);
          await client.query(`
            INSERT INTO engine_pricing_jobs (
              id, user_id, engine_account_id, multiplier_bp, reason
            ) VALUES ($1, $2, $3, $4, 'b2c_window_downgrade')
            ON CONFLICT (user_id) DO UPDATE SET
              engine_account_id = EXCLUDED.engine_account_id,
              multiplier_bp = EXCLUDED.multiplier_bp,
              reason = EXCLUDED.reason,
              status = 'pending', attempts = 0, next_attempt_at = now(),
              locked_at = NULL, locked_by = NULL, last_error = NULL,
              confirmed_at = NULL, updated_at = now()
          `, [randomUUID(), row.user_id, row.engine_account_id, nextRule.multiplierBp]);
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

  private async reverseRefundedTopups(): Promise<number> {
    let reversed = 0;
    for (;;) {
      const client = await this.database.pool.connect();
      try {
        await client.query("BEGIN");
        const result = await client.query<{
          payment_id: string;
          user_id: string;
          amount_nano: string;
          current_tier: number;
          cumulative_topup_nano: string;
          engine_account_id: string;
        }>(`
          SELECT p.id AS payment_id, p.user_id, p.amount_nano, cp.current_tier,
                 cp.cumulative_topup_nano, ea.engine_account_id
          FROM payments p
          JOIN engine_credits credit ON credit.payment_id = p.id AND credit.status = 'confirmed'
          JOIN customer_profiles cp ON cp.user_id = p.user_id AND cp.customer_type = 'b2c'
          JOIN engine_accounts ea ON ea.user_id = p.user_id
          WHERE p.status = 'refunded'
            AND ea.engine_account_id IS NOT NULL
            AND cp.cumulative_topup_nano >= p.amount_nano
            AND NOT EXISTS (
              SELECT 1 FROM webhook_events marker
              WHERE marker.provider = $1
                AND marker.provider_event_id = 'refund-reversal:' || p.id::text
            )
          ORDER BY p.updated_at, p.id
          FOR UPDATE OF p, cp, ea SKIP LOCKED
          LIMIT 1
        `, [REFUND_REVERSAL_PROVIDER]);
        const row = result.rows[0];
        if (!row) {
          await client.query("COMMIT");
          return reversed;
        }

        // C21: this unique synthetic event is the durable, transactional idempotency guard. A
        // refund can be observed on every poll, but its top-up contribution is removed only once.
        const guard = await client.query<{ id: string }>(`
          INSERT INTO webhook_events (
            id, provider, provider_event_id, event_type, payload, status, attempts, processed_at
          ) VALUES ($1, $2, $3, 'pricing.refund_reversal', $4::jsonb, 'processed', 1, now())
          ON CONFLICT (provider, provider_event_id) DO NOTHING
          RETURNING id
        `, [
          randomUUID(),
          REFUND_REVERSAL_PROVIDER,
          `refund-reversal:${row.payment_id}`,
          JSON.stringify({ paymentId: row.payment_id, userId: row.user_id, amountNano: row.amount_nano }),
        ]);
        if (!guard.rows[0]) {
          await client.query("COMMIT");
          continue;
        }

        const cumulativeTopupNano = BigInt(row.cumulative_topup_nano) - BigInt(row.amount_nano);
        const topupTier = tierForTopups(cumulativeTopupNano);
        const nextTier = Math.min(row.current_tier, topupTier);
        const nextRule = B2C_TIER_RULES[nextTier]!;
        const tierChanged = nextTier !== row.current_tier;

        await client.query(`
          UPDATE customer_profiles
          SET cumulative_topup_nano = $2,
              current_tier = $3,
              multiplier_bp = $4,
              tier_window_start = CASE WHEN $5 THEN $6 ELSE tier_window_start END,
              tier_window_spent_nano = CASE WHEN $5 THEN 0 ELSE tier_window_spent_nano END,
              updated_at = now()
          WHERE user_id = $1 AND customer_type = 'b2c'
        `, [
          row.user_id,
          cumulativeTopupNano.toString(),
          nextTier,
          nextRule.multiplierBp,
          tierChanged,
          nextTier > 0 ? new Date() : null,
        ]);

        if (tierChanged) {
          await client.query(`
            UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
          `, [row.user_id, nextRule.multiplierBp]);
          await client.query(`
            INSERT INTO engine_pricing_jobs (
              id, user_id, engine_account_id, multiplier_bp, reason
            ) VALUES ($1, $2, $3, $4, 'b2c_refund_reversal')
            ON CONFLICT (user_id) DO UPDATE SET
              engine_account_id = EXCLUDED.engine_account_id,
              multiplier_bp = EXCLUDED.multiplier_bp,
              reason = EXCLUDED.reason,
              status = 'pending', attempts = 0, next_attempt_at = now(),
              locked_at = NULL, locked_by = NULL, last_error = NULL,
              confirmed_at = NULL, updated_at = now()
          `, [randomUUID(), row.user_id, row.engine_account_id, nextRule.multiplierBp]);
        }

        // AUDIT-TODO(C21): run pnpm db:generate + migrate after adding an immutable top-up
        // contribution/reversal table keyed by payment_id. That removes the remaining ambiguity when
        // an older retention downgrade already reduced cumulative_topup_nano below a refund amount.
        await client.query("COMMIT");
        reversed += 1;
      } catch (error) {
        await client.query("ROLLBACK");
        throw error;
      } finally {
        client.release();
      }
    }
  }

  private async flushPricingJobs(): Promise<void> {
    for (;;) {
      const job = await claimNextPricingJob(this.database, this.workerId);
      if (!job) return;
      try {
        await this.engine.setAccountMultiplier(job.engineAccountId, job.multiplierBp);
        await confirmPricingJob(this.database, job);
      } catch (error) {
        try {
          await retryPricingJob(this.database, job, message(error));
        } catch (retryError) {
          // C68: periodic stale-lease recovery above guarantees this processing job becomes
          // claimable again even if persisting its retry state failed during a database outage.
          // AUDIT-TODO(C68): move lease-expiry reclamation into claimNextPricingJob itself.
          this.logger.error(`failed to release pricing job ${job.id}: ${message(retryError)}`);
          throw retryError;
        }
      }
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([
      new Promise((resolve) => setTimeout(resolve, milliseconds)),
      this.stopSignal,
    ]);
  }
}

function afterMonthCloseGrace(now: Date, graceMs: number): boolean {
  return now.getTime() >= utcMonthStart(now).getTime() + graceMs;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "pricing worker failed";
}
