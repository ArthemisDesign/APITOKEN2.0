import { Inject, Injectable } from "@nestjs/common";
import {
  getAdminEmailOutboxHealth,
  getAdminEngineCreditHealth,
  getAdminPricingJobHealth,
  getAdminWebhookHealth,
  type Database,
} from "@claude-api/db";
import { DATABASE } from "./infrastructure.module.js";
import { nanoToUsd } from "./admin-operations.service.js";

/**
 * GET /admin/pipeline-health: read-only сводка сбоев денежных пайплайнов commerce
 * (engine_credits → webhook_events → email_outbox → engine_pricing_jobs). Деньги — строки
 * integer nano-USD (инвариант проекта: без float/JS number), рядом usd-строки через тот же
 * точный nanoToUsd, что у соседних admin-эндпоинтов. Даты — ISO 8601; при пустых таблицах
 * возраст/даты — null, а не NaN.
 *
 * Вердикт ok/warn/bad по зафиксированным простым порогам (менять — только вместе с этим
 * комментарием и тестами):
 * - bad: engine_credits.dead_count > 0 (хотя бы один кредит терминально мёртв — деньги
 *   клиента оплачены, но не зачислены и автоматика уже сдалась) ИЛИ
 *   webhook_events.failed_24h > 0 (свежая, за последние 24 часа, потеря платёжного события
 *   провайдера — прямой риск незачисленного платежа);
 * - warn: backlog retry — engine_credits в status='retry' > 0 ИЛИ engine_pricing_jobs в
 *   status='retry' > 0 (автоматика ещё перепробует сама, но очередь копится) ИЛИ
 *   email_outbox.failed_total > 0 (терминально недоставленные письма клиентам — до чистки/
 *   переотправки сигнал держится, как и dead-кредиты держат bad);
 * - ok: ничего из перечисленного.
 * dead-кредит и failed-вебхук не «протухают» со временем: вердикт остаётся пониженным, пока
 * оператор не разберёт строки. Причины вердикта отдаются в verdict_reasons с числами.
 */
@Injectable()
export class AdminPipelinesService {
  constructor(@Inject(DATABASE) private readonly database: Database) {}

  async pipelineHealth(): Promise<Record<string, unknown>> {
    const [credits, webhooks, email, pricingJobs] = await Promise.all([
      getAdminEngineCreditHealth(this.database),
      getAdminWebhookHealth(this.database),
      getAdminEmailOutboxHealth(this.database),
      getAdminPricingJobHealth(this.database),
    ]);

    const badReasons: string[] = [];
    if (credits.countsByStatus.dead > 0) {
      badReasons.push(`engine_credits: ${credits.countsByStatus.dead} dead`);
    }
    if (webhooks.failed24h > 0) {
      badReasons.push(`webhook_events: ${webhooks.failed24h} failed in last 24h`);
    }
    const warnReasons: string[] = [];
    if (credits.countsByStatus.retry > 0) {
      warnReasons.push(`engine_credits: ${credits.countsByStatus.retry} in retry`);
    }
    if (pricingJobs.countsByStatus.retry > 0) {
      warnReasons.push(`engine_pricing_jobs: ${pricingJobs.countsByStatus.retry} in retry`);
    }
    if (email.failedTotal > 0) {
      warnReasons.push(`email_outbox: ${email.failedTotal} failed`);
    }
    const verdict = badReasons.length > 0 ? "bad" : warnReasons.length > 0 ? "warn" : "ok";

    return {
      generated_at: new Date().toISOString(),
      verdict,
      verdict_reasons: [...badReasons, ...warnReasons],
      engine_credits: {
        counts_by_status: credits.countsByStatus,
        dead_count: credits.countsByStatus.dead,
        retry_high_attempts_count: credits.retryHighAttempts,
        oldest_unconfirmed_created_at:
          credits.oldestUnconfirmedCreatedAt?.toISOString() ?? null,
        oldest_unconfirmed_age_seconds: credits.oldestUnconfirmedAgeSeconds,
        stuck_nano: credits.stuckNano,
        stuck_usd: nanoToUsd(credits.stuckNano),
      },
      webhook_events: {
        failed_total: webhooks.failedTotal,
        failed_24h: webhooks.failed24h,
        recent_failures: webhooks.recentFailures.map((row) => ({
          provider: row.provider,
          event_type: row.eventType,
          attempts: row.attempts,
          received_at: row.receivedAt.toISOString(),
          last_error: row.lastError,
        })),
      },
      email_outbox: {
        failed_total: email.failedTotal,
        recent_failures: email.recentFailures.map((row) => ({
          template: row.template,
          attempts: row.attempts,
          last_error: row.lastError,
        })),
      },
      engine_pricing_jobs: {
        counts_by_status: pricingJobs.countsByStatus,
        retry_count: pricingJobs.countsByStatus.retry,
        oldest_unconfirmed_created_at:
          pricingJobs.oldestUnconfirmedCreatedAt?.toISOString() ?? null,
        oldest_unconfirmed_age_seconds: pricingJobs.oldestUnconfirmedAgeSeconds,
        recent_errors: pricingJobs.recentErrors.map((row) => ({
          user_id: row.userId,
          engine_account_id: row.engineAccountId,
          reason: row.reason,
          status: row.status,
          attempts: row.attempts,
          last_error: row.lastError,
        })),
      },
    };
  }
}
