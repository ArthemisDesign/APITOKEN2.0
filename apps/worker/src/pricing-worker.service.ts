import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  applyPricingLedgerPage,
  claimNextPricingJob,
  closeElapsedTierWindows,
  confirmPricingJob,
  getPricingUsageCursor,
  listPricingSyncTargets,
  recoverStalePricingJobs,
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
        const targets = await listPricingSyncTargets(this.database);
        for (const target of targets) {
          if (this.stopped) break;
          try {
            await this.syncTarget(target);
          } catch (error) {
            this.logger.error(`pricing usage sync failed for ${target.userId}: ${message(error)}`);
          }
        }
        if (afterMonthCloseGrace(new Date(), this.config.get("PRICING_CLOSE_GRACE_MS", { infer: true }))) {
          const closed = await closeElapsedTierWindows(this.database);
          if (closed > 0) this.logger.log(`closed ${closed} elapsed pricing months`);
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
    for (;;) {
      const entries = await this.engine.getLedgerAfter(target.engineAccountId, cursor, 1000);
      if (entries.length === 0) return;
      await applyPricingLedgerPage(this.database, target, entries);
      cursor = BigInt(entries.at(-1)!.id);
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
        await retryPricingJob(this.database, job, message(error));
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
