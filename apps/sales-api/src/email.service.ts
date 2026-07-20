import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import nodemailer, { type Transporter } from "nodemailer";
import {
  claimNextPartnerEmail,
  confirmPartnerEmail,
  decodeSalesEncryptionKey,
  decryptSalesToken,
  recoverStalePartnerEmails,
  retryPartnerEmail,
  type ClaimedPartnerEmail,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { renderPartnerEmail } from "./email-template.js";
import { SALES_DATABASE, WORKER_ID } from "./infrastructure.module.js";
import { smtpSecurityOptions } from "./smtp.js";

const STALE_EMAIL_RECOVERY_INTERVAL_MS = 60_000;

@Injectable()
export class EmailService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(EmailService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private transport: Transporter | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });

  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    if (this.config.get("EMAIL_DELIVERY_MODE", { infer: true }) === "smtp") {
      const environment = {
        NODE_ENV: this.config.get("NODE_ENV", { infer: true }),
        SMTP_SECURE: this.config.get("SMTP_SECURE", { infer: true }),
      };
      this.transport = nodemailer.createTransport({
        host: this.config.get("SMTP_HOST", { infer: true }),
        port: this.config.get("SMTP_PORT", { infer: true }),
        ...smtpSecurityOptions(environment),
        auth: this.config.get("SMTP_USERNAME", { infer: true }) ? {
          user: this.config.get("SMTP_USERNAME", { infer: true }),
          pass: this.config.get("SMTP_PASSWORD", { infer: true }),
        } : undefined,
      });
    }
    const recovered = await recoverStalePartnerEmails(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale email jobs`);
    this.loop = this.run().catch((error) => {
      this.logger.error(`email delivery loop terminated unexpectedly: ${message(error)}`);
    });
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
    this.transport?.close();
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("EMAIL_POLL_INTERVAL_MS", { infer: true });
    let nextRecoveryAt = Date.now() + STALE_EMAIL_RECOVERY_INTERVAL_MS;
    this.logger.log(`email delivery loop ${this.workerId} started (${this.config.get("EMAIL_DELIVERY_MODE", { infer: true })} mode)`);
    while (!this.stopped) {
      try {
        if (Date.now() >= nextRecoveryAt) {
          const recovered = await recoverStalePartnerEmails(this.database);
          if (recovered > 0) this.logger.warn(`recovered ${recovered} stale email jobs`);
          nextRecoveryAt = Date.now() + STALE_EMAIL_RECOVERY_INTERVAL_MS;
        }

        const job = await claimNextPartnerEmail(this.database, this.workerId);
        if (!job) {
          await this.sleep(pollMs);
          continue;
        }
        try {
          await this.send(job);
          await confirmPartnerEmail(this.database, job.id, this.workerId);
        } catch (error) {
          const deliveryError = message(error);
          this.logger.error(`email ${job.id} delivery failed: ${deliveryError}`);
          await retryPartnerEmail(this.database, job, deliveryError, this.workerId);
        }
      } catch (error) {
        this.logger.error(`email delivery iteration failed: ${message(error)}`);
        await this.sleep(pollMs);
      }
    }
  }

  private async send(job: ClaimedPartnerEmail): Promise<void> {
    const key = decodeSalesEncryptionKey(this.config.get("SALES_TOKEN_ENCRYPTION_KEY", { infer: true }));
    const token = decryptSalesToken(job.encryptedToken, key);
    const content = renderPartnerEmail(job.template, token, this.config.get("PUBLIC_SALES_BASE_URL", { infer: true }));
    if (this.config.get("EMAIL_DELIVERY_MODE", { infer: true }) === "log") {
      // Never log the token or the action URL — only non-secret delivery metadata.
      this.logger.log(`[log delivery] email ${job.id} (${job.template}) would be sent to ${job.recipient}`);
      return;
    }
    if (!this.transport) throw new Error("SMTP transport is unavailable");
    await this.transport.sendMail({
      from: this.config.get("EMAIL_FROM", { infer: true }),
      to: job.recipient,
      subject: content.subject,
      text: content.text,
      html: content.html,
    });
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([new Promise((resolve) => setTimeout(resolve, milliseconds)), this.stopSignal]);
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "email delivery failed";
}
