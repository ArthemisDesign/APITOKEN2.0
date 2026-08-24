import { Injectable, Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";

export interface PartnerApplicationWebhookPayload {
  event: "submitted" | "updated" | "decided";
  id: string;
  email: string;
  status: "pending" | "approved" | "rejected";
  message: string;
  createdAt: string;
  reviewerActor: string | null;
  reviewerNote: string | null;
}

const TIMEOUT_MS = 2_000;

/**
 * Fail-open loopback notify for partner-access applications. Disabled when the URL is unset.
 * Logs only the application id — never the webhook URL or the applicant message.
 */
@Injectable()
export class DevbotPartnerNotifier {
  private readonly logger = new Logger(DevbotPartnerNotifier.name);

  constructor(private readonly config: ConfigService<Environment, true>) {}

  notify(payload: PartnerApplicationWebhookPayload): void {
    const url = this.config.get("DEVBOT_PARTNER_WEBHOOK_URL", { infer: true });
    if (url === undefined) return;
    void this.post(url, payload);
  }

  private async post(url: string, payload: PartnerApplicationWebhookPayload): Promise<void> {
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload),
        signal: AbortSignal.timeout(TIMEOUT_MS),
      });
      if (!response.ok) {
        this.logger.warn(`devbot partner notify failed: application ${payload.id} HTTP ${response.status}`);
      }
    } catch {
      this.logger.warn(`devbot partner notify failed: application ${payload.id}`);
    }
  }
}
