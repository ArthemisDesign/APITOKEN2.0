import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  applyPricingLedgerPage,
  claimNextPricingJob,
  closeElapsedTierWindows,
  completePricingUsageSync,
  confirmPricingJob,
  getPricingUsageCursor,
  listPricingSyncTargets,
  reconcileTierLadderMultipliers,
  recoverStalePricingJobs,
  refreshTierWindowUsage,
  retryPricingJob,
  utcMonthStart,
  type Database,
  type PricingSyncTarget,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

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
    // После изменения констант лестницы существующие профили сходятся к ней на первом старте;
    // engine получает новые множители через обычные durable pricing jobs в flushPricingJobs.
    const reconciled = await reconcileTierLadderMultipliers(this.database);
    if (reconciled > 0) this.logger.warn(`reconciled ${reconciled} b2c profiles to current tier ladder`);
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

        const now = new Date();
        // getPricingUsageCursor reconciles durable confirmed-credit accrual markers, including
        // refund/dispute reversal, before any engine network I/O. Keep one authority for that
        // mutation: a separate worker-side refund loop can subtract the same marker twice.
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
        await refreshTierWindowUsage(this.database, syncedUserIds, now);

        if (
          syncedUserIds.length > 0 &&
          afterMonthCloseGrace(now, this.config.get("PRICING_CLOSE_GRACE_MS", { infer: true }))
        ) {
          // C19: only users whose ledger sync completed in this cycle are eligible for closure.
          // AUDIT-TODO(C19): run pnpm db:generate + migrate after adding a durable per-account
          // engine cutoff watermark; require it to cover the exact window end before closure.
          const closed = await closeElapsedTierWindows(this.database, now, syncedUserIds);
          if (closed > 0) {
            await refreshTierWindowUsage(this.database, syncedUserIds, now);
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
      if (entries.length === 0) {
        // An empty page is also a completed ledger scan. getPricingUsageCursor invalidates the
        // previous completion marker before network I/O, so restore it only after this response.
        await completePricingUsageSync(this.database, target);
        return;
      }
      await applyPricingLedgerPage(this.database, target, entries);
      cursor = BigInt(entries.at(-1)!.id);
      await this.engine.acknowledgeLedger(target.engineAccountId, cursor);
      if (entries.length < 1000) return;
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
