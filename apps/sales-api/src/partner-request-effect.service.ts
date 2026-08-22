import {
  Inject,
  Injectable,
  Logger,
  type OnApplicationShutdown,
  type OnModuleInit,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  claimPartnerRequestEffect,
  markPartnerRequestEffectApplied,
  markPartnerRequestEffectFailed,
  recoverStalePartnerRequestEffects,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { CommercePartnerPricingError, CommerceService } from "./commerce.service.js";
import type { Environment } from "./config.js";
import { SALES_DATABASE, WORKER_ID } from "./infrastructure.module.js";

const RECOVERY_INTERVAL_MS = 30_000;

@Injectable()
export class PartnerRequestEffectService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(PartnerRequestEffectService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });

  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
    private readonly commerce: CommerceService,
  ) {}

  async onModuleInit(): Promise<void> {
    const recovered = await recoverStalePartnerRequestEffects(
      this.database,
      this.config.get("PARTNER_EFFECT_LEASE_SECONDS", { infer: true }),
    );
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale partner request effects`);
    this.loop = this.run().catch((error) => {
      this.logger.error(`partner request effect loop terminated unexpectedly: ${message(error)}`);
    });
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("PARTNER_EFFECT_POLL_INTERVAL_MS", { infer: true });
    const leaseSeconds = this.config.get("PARTNER_EFFECT_LEASE_SECONDS", { infer: true });
    let nextRecoveryAt = Date.now() + RECOVERY_INTERVAL_MS;
    this.logger.log(`partner request effect loop ${this.workerId} started`);
    while (!this.stopped) {
      try {
        if (Date.now() >= nextRecoveryAt) {
          const recovered = await recoverStalePartnerRequestEffects(this.database, leaseSeconds);
          if (recovered > 0) this.logger.warn(`recovered ${recovered} stale partner request effects`);
          nextRecoveryAt = Date.now() + RECOVERY_INTERVAL_MS;
        }
        const effect = await claimPartnerRequestEffect(this.database, this.workerId);
        if (!effect) {
          await this.sleep(pollMs);
          continue;
        }
        try {
          const result = await this.commerce.setPartnerBusinessPricing(effect.payload);
          if (result.operationRef !== effect.payload.operationRef) {
            throw new CommercePartnerPricingError(502, "Commerce acknowledged another operation ref");
          }
          const fenced = await markPartnerRequestEffectApplied(this.database, {
            effectId: effect.effectId,
            requestId: effect.requestId,
            leaseToken: effect.leaseToken,
            commerceOperationRef: result.operationRef,
            idempotentReplay: result.idempotentReplay,
          });
          if (!fenced) this.logger.warn(`lost lease while applying partner request ${effect.requestId}`);
        } catch (error) {
          const terminal = isTerminalCommerceError(error);
          const retryAfterSeconds = Math.min(300, 2 ** Math.min(effect.attempts, 8));
          const fenced = await markPartnerRequestEffectFailed(this.database, {
            effectId: effect.effectId,
            requestId: effect.requestId,
            leaseToken: effect.leaseToken,
            error: message(error),
            retryAfterSeconds,
            terminal,
          });
          if (!fenced) this.logger.warn(`lost lease while failing partner request ${effect.requestId}`);
          else if (terminal) this.logger.error(`partner request ${effect.requestId} failed terminally: ${message(error)}`);
          else this.logger.warn(`partner request ${effect.requestId} will retry in ${retryAfterSeconds}s: ${message(error)}`);
        }
      } catch (error) {
        this.logger.error(`partner request effect iteration failed: ${message(error)}`);
        await this.sleep(pollMs);
      }
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([new Promise((resolve) => setTimeout(resolve, milliseconds)), this.stopSignal]);
  }
}

export function isTerminalCommerceError(error: unknown): boolean {
  return error instanceof CommercePartnerPricingError
    && (error.status === 400 || error.status === 403 || error.status === 409);
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Commerce partner pricing failed";
}
