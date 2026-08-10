import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  applyPricingLedgerPage,
  applyPricingProviderBackfillPage,
  applyPricingTopupBackfillPage,
  claimNextPricingJob,
  completePricingProviderBackfill,
  completePricingUsageSync,
  confirmPricingJob,
  getPricingUsageCursor,
  getPricingProviderBackfillCursor,
  getPricingTopupBackfillCursor,
  listPricingSyncTargets,
  recoverStalePricingJobs,
  retryPricingJob,
  type Database,
  type PricingSyncTarget,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

const PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC = 4;
// Догоняющий скан истории пополнений — разовая работа на аккаунт, поэтому лимит страниц за цикл
// держим таким же скромным: цель не «быстро», а «без всплеска нагрузки на движок».
const TOPUP_BACKFILL_MAX_PAGES_PER_SYNC = 4;

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
    const dispatchMs = this.config.get("PRICING_DISPATCH_MS", { infer: true });
    // Job delivery and the fleet sweep are different workloads on the same loop. Delivery is a
    // cheap indexed claim and is what a newly provisioned account's first dashboard load blocks
    // on; the sweep walks every pricing target doing per-user engine I/O. Running delivery only
    // once per sweep made a signup wait for the whole sweep plus the poll interval, so the wait
    // grew with the size of the fleet. Deliver on a short tick, sweep on the slow one.
    let nextSweepAt = 0;
    this.logger.log(
      `pricing worker ${this.workerId} started (dispatch ${dispatchMs}ms, sweep ${pollMs}ms)`,
    );
    while (!this.stopped) {
      try {
        // Delivery first and every tick: never behind the sweep — a discount change must reach
        // the engine without waiting for the fleet-wide usage sweep.
        await this.flushPricingJobs();
      } catch (error) {
        this.logger.error(message(error));
      }
      if (Date.now() < nextSweepAt) {
        await this.sleep(dispatchMs);
        continue;
      }
      nextSweepAt = Date.now() + pollMs;
      try {
        // C68: recovery is part of normal polling, so a failed retry-state write cannot strand a
        // processing lease until process restart.
        const recovered = await recoverStalePricingJobs(this.database);
        if (recovered > 0) this.logger.warn(`recovered ${recovered} stale pricing jobs`);

        // getPricingUsageCursor reconciles durable confirmed-credit accrual markers, including
        // refund/dispute reversal, before any engine network I/O. Keep one authority for that
        // mutation: a separate worker-side refund loop can subtract the same marker twice.
        const targets = await listPricingSyncTargets(this.database);
        for (const target of targets) {
          if (this.stopped) break;
          try {
            await this.syncTarget(target);
          } catch (error) {
            this.logger.error(`pricing usage sync failed for ${target.userId}: ${message(error)}`);
          }
        }
      } catch (error) {
        this.logger.error(message(error));
      }
      await this.sleep(dispatchMs);
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
        break;
      }
      await applyPricingLedgerPage(this.database, target, entries);
      cursor = BigInt(entries.at(-1)!.id);
      await this.engine.acknowledgeLedger(target.engineAccountId, cursor);
      if (entries.length < 1000) break;
    }
    const providerRows = await this.backfillTargetProviders(target, cursor);
    if (providerRows > 0) {
      this.logger.log(`completed provider recovery for ${providerRows} usage rows of ${target.userId}`);
    }
    const topupRows = await this.backfillTargetTopups(target, cursor);
    if (topupRows > 0) {
      this.logger.log(`recorded ${topupRows} historical engine top-ups of ${target.userId}`);
    }
  }

  /**
   * История пополнений: обычный курсор расхода уже стоит выше старых топапов, поэтому отчётная
   * таблица заполняется отдельным маркером с начала леджера — один раз на аккаунт, ограниченным
   * числом страниц за цикл.
   */
  private async backfillTargetTopups(
    target: PricingSyncTarget,
    throughLedgerId: bigint,
  ): Promise<number> {
    const start = await getPricingTopupBackfillCursor(this.database, target, throughLedgerId);
    if (start === null) return 0;
    let cursor: bigint = start;
    let recorded = 0;
    for (let page = 0; page < TOPUP_BACKFILL_MAX_PAGES_PER_SYNC; page += 1) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) {
        recorded += await applyPricingTopupBackfillPage(this.database, target, [], throughLedgerId);
        return recorded;
      }
      const nextCursor = entries.reduce<bigint>((highest, entry) => {
        const ledgerId = BigInt(entry.id);
        return ledgerId > highest ? ledgerId : highest;
      }, cursor);
      if (nextCursor <= cursor) throw new Error("engine top-up backfill ledger page did not advance");
      const terminal = nextCursor >= throughLedgerId || entries.length < 1000;
      recorded += await applyPricingTopupBackfillPage(
        this.database,
        target,
        entries,
        terminal ? throughLedgerId : nextCursor,
      );
      cursor = nextCursor;
      if (terminal) return recorded;
    }
    return recorded;
  }

  private async backfillTargetProviders(
    target: PricingSyncTarget,
    throughLedgerId: bigint,
  ): Promise<number> {
    const backfillCursor = await getPricingProviderBackfillCursor(
      this.database,
      target,
      throughLedgerId,
    );
    if (backfillCursor === null) return 0;
    let cursor: bigint = backfillCursor;

    let resolved = 0;
    for (let page = 0; page < PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC; page += 1) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) {
        return resolved + await completePricingProviderBackfill(
          this.database,
          target,
          throughLedgerId,
        );
      }
      resolved += await applyPricingProviderBackfillPage(this.database, target, entries);
      const nextCursor = entries.reduce<bigint>((highest, entry) => {
        const ledgerId = BigInt(entry.id);
        return ledgerId > highest ? ledgerId : highest;
      }, cursor);
      if (nextCursor <= cursor) {
        throw new Error("engine provider backfill ledger page did not advance");
      }
      cursor = nextCursor;
      const terminal = cursor >= throughLedgerId || entries.length < 1000;
      resolved += await completePricingProviderBackfill(
        this.database,
        target,
        terminal ? throughLedgerId : cursor,
      );
      if (terminal) return resolved;
    }
    return resolved;
  }


  private async flushPricingJobs(): Promise<void> {
    for (;;) {
      const job = await claimNextPricingJob(this.database, this.workerId);
      if (!job) return;
      try {
        // One job, one target: the account default, or one provider's override (a null
        // multiplier there removes the override and returns that provider to the default).
        if (job.providerId === null) {
          if (job.multiplierBp === null) throw new Error("account pricing job carries no multiplier");
          await this.engine.setAccountMultiplier(job.engineAccountId, job.multiplierBp);
        } else {
          await this.engine.setAccountProviderDiscount(
            job.engineAccountId,
            job.providerId,
            job.multiplierBp,
          );
        }
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

function message(error: unknown): string {
  return error instanceof Error ? error.message : "pricing worker failed";
}
