import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import nodemailer, { type Transporter } from "nodemailer";
import {
  claimNextEmail,
  confirmEmail,
  decodeAuthEncryptionKey,
  decryptAuthToken,
  recoverStaleEmails,
  retryEmail,
  type ClaimedEmail,
  type Database,
} from "@claude-api/db";
import type { Environment } from "./config.js";
import { renderAuthEmail, renderBusinessInviteEmail } from "./email-template.js";
import { smtpSecurityOptions } from "./smtp.js";
import { DATABASE, WORKER_ID } from "./tokens.js";

const STALE_EMAIL_RECOVERY_INTERVAL_MS = 60_000;

@Injectable()
export class EmailWorkerService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(EmailWorkerService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private transport: Transporter | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(WORKER_ID) private readonly workerId: string,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async onModuleInit(): Promise<void> {
    if (this.config.get("EMAIL_DELIVERY_MODE", { infer: true }) === "disabled") {
      this.logger.warn("email delivery is disabled; messages will remain queued");
      return;
    }
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
    const recovered = await recoverStaleEmails(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale email jobs`);
    this.loop = this.run().catch((error) => {
      this.stopped = true;
      this.logger.error(`email worker terminated unexpectedly: ${message(error)}`);
      process.exitCode = 1;
      process.kill(process.pid, "SIGTERM");
    });
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
    this.transport?.close();
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("EMAIL_POLL_MS", { infer: true });
    let nextRecoveryAt = Date.now() + STALE_EMAIL_RECOVERY_INTERVAL_MS;
    this.logger.log(`email worker ${this.workerId} started`);
    while (!this.stopped) {
      try {
        if (Date.now() >= nextRecoveryAt) {
          const recovered = await recoverStaleEmails(this.database);
          if (recovered > 0) this.logger.warn(`recovered ${recovered} stale email jobs`);
          nextRecoveryAt = Date.now() + STALE_EMAIL_RECOVERY_INTERVAL_MS;
        }

        const job = await claimNextEmail(this.database, this.workerId);
        if (!job) {
          await this.sleep(pollMs);
          continue;
        }
        try {
          const messageId = await this.send(job);
          await confirmEmail(this.database, job.id, messageId);
        } catch (error) {
          const deliveryError = message(error);
          this.logger.error(`email ${job.id} delivery failed: ${deliveryError}`);
          await retryEmail(this.database, job, deliveryError);
        }
      } catch (error) {
        this.logger.error(`email worker iteration failed: ${message(error)}`);
        await this.sleep(pollMs);
      }
    }
  }

  private async send(job: ClaimedEmail): Promise<string> {
    if (!this.transport) throw new Error("SMTP transport is unavailable");
    const key = decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
    const token = decryptAuthToken(job.encryptedToken, key);
    const appBaseUrl = this.config.get("PUBLIC_APP_BASE_URL", { infer: true });
    const content = job.template === "business_invite"
      ? renderBusinessInviteEmail(
        token,
        appBaseUrl,
        job.payload.pricingPolicy === "provider_model"
          ? null
          : readNumber(job.payload, "discountPercent"),
        readString(job.payload, "expiresAt"),
      )
      : renderAuthEmail(job.template, token, appBaseUrl);
    const result = await this.transport.sendMail({
      from: this.config.get("EMAIL_FROM", { infer: true }),
      to: job.recipient,
      subject: content.subject,
      text: content.text,
      html: content.html,
    });
    return result.messageId || `smtp:${job.id}`;
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([new Promise((resolve) => setTimeout(resolve, milliseconds)), this.stopSignal]);
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "email delivery failed";
}

function readString(payload: Record<string, unknown>, key: string): string {
  const value = payload[key];
  if (typeof value !== "string") throw new Error(`email payload has no ${key}`);
  return value;
}

function readNumber(payload: Record<string, unknown>, key: string): number {
  const value = payload[key];
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`email payload has no ${key}`);
  return value;
}
