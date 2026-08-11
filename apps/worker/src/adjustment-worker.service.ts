import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  claimNextAdjustment,
  confirmAdjustment,
  recoverStaleAdjustments,
  retryAdjustment,
  type Database,
} from "@claude-api/db";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

@Injectable()
export class AdjustmentWorkerService implements OnModuleInit, OnApplicationShutdown {
  private static readonly STALE_RECOVERY_INTERVAL_MS = 60_000;

  private readonly logger = new Logger(AdjustmentWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    await this.recoverStaleJobs();
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("CREDIT_POLL_MS", { infer: true });
    let nextRecoveryAt = Date.now() + AdjustmentWorkerService.STALE_RECOVERY_INTERVAL_MS;
    this.logger.log(`adjustment worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        if (Date.now() >= nextRecoveryAt) {
          await this.recoverStaleJobs();
          nextRecoveryAt = Date.now() + AdjustmentWorkerService.STALE_RECOVERY_INTERVAL_MS;
        }

        const adjustment = await claimNextAdjustment(this.database, this.workerId);
        if (!adjustment) {
          await this.sleep(pollMs);
          continue;
        }
        try {
          const result = await this.engine.debitAccount(
            adjustment.engineAccountId,
            adjustment.amountNano,
            adjustment.idempotencyRef,
          );
          const confirmed = await confirmAdjustment(
            this.database,
            adjustment.id,
            adjustment.leaseToken,
            BigInt(result.balance_nano),
          );
          if (!confirmed) {
            this.logger.warn(
              `adjustment ${adjustment.id} was not confirmed because its worker lease is no longer owned`,
            );
            continue;
          }
          this.logger.log(`confirmed adjustment ${adjustment.id}`);
        } catch (error) {
          const errorMessage = error instanceof Error ? error.message : "unknown adjustment error";
          if (error instanceof EngineClientError && !error.retryable) {
            // A mapping/reference conflict needs operator repair, but the refund is already
            // authoritative. Keep the durable debit retryable instead of losing compensation.
            this.logger.error(
              `engine rejected adjustment ${adjustment.id}; retaining for retry: ${errorMessage}`,
            );
          }
          try {
            const released = await retryAdjustment(
              this.database,
              adjustment.id,
              adjustment.leaseToken,
              errorMessage,
              adjustment.attempts,
            );
            if (!released) {
              this.logger.warn(
                `adjustment ${adjustment.id} retry state was not updated because its worker lease is no longer owned`,
              );
            }
          } catch (retryError) {
            const retryMessage = retryError instanceof Error
              ? retryError.message
              : "unknown database error";
            throw new Error(
              `failed to persist retry state for adjustment ${adjustment.id}: ${retryMessage}`,
            );
          }
        }
      } catch (error) {
        this.logger.error(error instanceof Error ? error.message : "adjustment worker loop failed");
        await this.sleep(pollMs);
      }
    }
  }

  private async recoverStaleJobs(): Promise<void> {
    const recovered = await recoverStaleAdjustments(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale adjustment jobs`);
  }

  private async sleep(milliseconds: number): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
  }
}
