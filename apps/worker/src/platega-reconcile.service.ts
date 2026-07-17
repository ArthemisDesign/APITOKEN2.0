import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { applyVerifiedCheckoutPaymentEvent, listPendingCheckoutsForReconcile, type Database } from "@claude-api/db";
import { PlategaError, PlategaProvider } from "@claude-api/payments";
import type { Environment } from "./config.js";
import { DATABASE } from "./tokens.js";

// Safety net for the webhook: periodically re-verify still-pending Platega checkouts against the
// provider and credit confirmed ones the callback never delivered. Crediting is idempotent — the
// event id (id:STATUS) dedups against webhook_events, so a webhook and this poller cannot double-credit.
@Injectable()
export class PlategaReconcileService implements OnModuleInit, OnApplicationShutdown {
  private static readonly MAX_AGE_SECONDS = 172_800; // stop re-querying checkouts older than 2 days
  private static readonly BATCH_LIMIT = 50;

  private readonly logger = new Logger(PlategaReconcileService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private provider: PlategaProvider | undefined;

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    const merchantId = this.config.get("PLATEGA_MERCHANT_ID", { infer: true });
    const secret = this.config.get("PLATEGA_SECRET", { infer: true });
    if (!merchantId || !secret) {
      this.logger.log("Platega reconcile disabled (no PLATEGA credentials configured)");
      return;
    }
    this.provider = new PlategaProvider({
      merchantId,
      secret,
      callbackUrl: new URL("/v1/payments/platega/webhook", this.config.get("PUBLIC_API_BASE_URL", { infer: true })).toString(),
      apiBaseUrl: this.config.get("PLATEGA_API_BASE_URL", { infer: true }),
    });
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    await this.loop;
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("PLATEGA_RECONCILE_MS", { infer: true });
    const minAgeSeconds = this.config.get("PLATEGA_RECONCILE_MIN_AGE_S", { infer: true });
    this.logger.log("Platega reconcile poller started");
    while (!this.stopped) {
      try {
        const rows = await listPendingCheckoutsForReconcile(this.database, {
          provider: "platega",
          minAgeSeconds,
          maxAgeSeconds: PlategaReconcileService.MAX_AGE_SECONDS,
          limit: PlategaReconcileService.BATCH_LIMIT,
        });
        for (const row of rows) {
          if (this.stopped) break;
          await this.reconcileOne(row);
        }
      } catch (error) {
        this.logger.error(error instanceof Error ? error.message : "Platega reconcile loop failed");
      }
      await this.sleep(pollMs);
    }
  }

  private async reconcileOne(row: { id: string; providerPaymentId: string; amountUsd: bigint }): Promise<void> {
    try {
      const payment = await this.provider!.verifyPayment(row.providerPaymentId);
      if (payment.state === "pending") return;
      if (payment.checkoutId && payment.checkoutId !== row.id) {
        this.logger.warn(`Platega payment ${row.providerPaymentId} payload does not match checkout ${row.id}; skipping`);
        return;
      }
      const applied = await applyVerifiedCheckoutPaymentEvent(this.database, {
        provider: "platega",
        providerEventId: payment.providerEventId,
        providerPaymentId: payment.providerPaymentId,
        checkoutId: row.id,
        state: payment.state,
        amountUsd: row.amountUsd,
        currency: "USD",
        paidAt: null,
        payload: payment.raw,
      });
      if (!applied.duplicateEvent) {
        this.logger.log(`reconciled Platega checkout ${row.id} (${row.providerPaymentId}) -> ${payment.state}`);
      }
    } catch (error) {
      if (error instanceof PlategaError && error.retryable) return; // transient; retry next cycle
      this.logger.error(`Platega reconcile ${row.providerPaymentId} failed: ${error instanceof Error ? error.message : "unknown"}`);
    }
  }

  private async sleep(milliseconds: number): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
  }
}
