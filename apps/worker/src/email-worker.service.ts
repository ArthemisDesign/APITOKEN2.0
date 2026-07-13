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
import { DATABASE, WORKER_ID } from "./tokens.js";

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
    this.transport = nodemailer.createTransport({
      host: this.config.get("SMTP_HOST", { infer: true }),
      port: this.config.get("SMTP_PORT", { infer: true }),
      secure: this.config.get("SMTP_SECURE", { infer: true }),
      auth: this.config.get("SMTP_USERNAME", { infer: true }) ? {
        user: this.config.get("SMTP_USERNAME", { infer: true }),
        pass: this.config.get("SMTP_PASSWORD", { infer: true }),
      } : undefined,
    });
    const recovered = await recoverStaleEmails(this.database);
    if (recovered > 0) this.logger.warn(`recovered ${recovered} stale email jobs`);
    this.loop = this.run();
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
    this.transport?.close();
  }

  private async run(): Promise<void> {
    const pollMs = this.config.get("EMAIL_POLL_MS", { infer: true });
    this.logger.log(`email worker ${this.workerId} started`);
    while (!this.stopped) {
      let job: ClaimedEmail | null;
      try {
        job = await claimNextEmail(this.database, this.workerId);
      } catch (error) {
        this.logger.error(`email claim failed: ${message(error)}`);
        await this.sleep(pollMs);
        continue;
      }
      if (!job) {
        await this.sleep(pollMs);
        continue;
      }
      try {
        const messageId = await this.send(job);
        await confirmEmail(this.database, job.id, messageId);
      } catch (error) {
        await retryEmail(this.database, job, message(error));
        this.logger.error(`email ${job.id} delivery failed: ${message(error)}`);
      }
    }
  }

  private async send(job: ClaimedEmail): Promise<string> {
    if (!this.transport) throw new Error("SMTP transport is unavailable");
    const key = decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
    const token = decryptAuthToken(job.encryptedToken, key);
    const content = renderEmail(job.template, token, this.config.get("PUBLIC_APP_BASE_URL", { infer: true }));
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

function renderEmail(template: ClaimedEmail["template"], token: string, appBaseUrl: string) {
  const path = template === "verify_email" ? "/verify-email" : "/reset-password";
  const url = new URL(path, appBaseUrl);
  url.searchParams.set("token", token);
  const title = template === "verify_email" ? "Verify your apitoken.sale email" : "Reset your apitoken.sale password";
  const action = template === "verify_email" ? "Verify email" : "Reset password";
  const escapedUrl = escapeHtml(url.toString());
  return {
    subject: title,
    text: `${title}\n\nOpen this link: ${url.toString()}\n\nIf you did not request this, ignore this email.`,
    html: `<h1>${escapeHtml(title)}</h1><p><a href="${escapedUrl}">${escapeHtml(action)}</a></p><p>If you did not request this, ignore this email.</p>`,
  };
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[character]!);
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "email delivery failed";
}
