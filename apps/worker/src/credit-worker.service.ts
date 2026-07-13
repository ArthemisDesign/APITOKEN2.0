import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  claimNextCredit,
  confirmCredit,
  recoverStaleCredits,
  retryCredit,
  type Database,
} from "@claude-api/db";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT, WORKER_ID } from "./tokens.js";

@Injectable()
export class CreditWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(CreditWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    const recovered = await recoverStaleCredits(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale credit jobs`);
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("CREDIT_POLL_MS", { infer: true });
    this.logger.log(`credit worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        const credit = await claimNextCredit(this.database, this.workerId);
        if (!credit) {
          await this.sleep(pollMs);
          continue;
        }
        try {
          const result = await this.engine.creditAccount(
            credit.engineAccountId,
            credit.amountNano,
            credit.idempotencyRef,
          );
          await confirmCredit(this.database, credit.id, BigInt(result.balance_nano));
          this.logger.log(`confirmed credit ${credit.id}`);
        } catch (error) {
          const message = error instanceof Error ? error.message : "unknown credit error";
          if (error instanceof EngineClientError && !error.retryable) {
            // Keep the job recoverable: an operator can fix account mapping or credentials without
            // risking a lost paid credit. Engine idempotency makes subsequent attempts safe.
            this.logger.error(`engine rejected credit ${credit.id}; retaining for retry: ${message}`);
          }
          await retryCredit(this.database, credit.id, message, credit.attempts);
        }
      } catch (error) {
        this.logger.error(error instanceof Error ? error.message : "worker loop failed");
        await this.sleep(pollMs);
      }
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
  }
}
