import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@claude-api/db", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@claude-api/db")>();
  return {
    ...actual,
    getAdminEmailOutboxHealth: vi.fn(),
    getAdminEngineCreditHealth: vi.fn(),
    getAdminPricingJobHealth: vi.fn(),
    getAdminWebhookHealth: vi.fn(),
  };
});

import {
  getAdminEmailOutboxHealth,
  getAdminEngineCreditHealth,
  getAdminPricingJobHealth,
  getAdminWebhookHealth,
  type AdminEmailOutboxHealth,
  type AdminEngineCreditHealth,
  type AdminPricingJobHealth,
  type AdminWebhookHealth,
} from "@claude-api/db";
import { AdminPipelinesService } from "./admin-pipelines.service.js";

const creditsMock = vi.mocked(getAdminEngineCreditHealth);
const webhooksMock = vi.mocked(getAdminWebhookHealth);
const emailMock = vi.mocked(getAdminEmailOutboxHealth);
const pricingJobsMock = vi.mocked(getAdminPricingJobHealth);

// db-слой замокан на уровне функций-репозиториев, поэтому Database не используется.
const service = new AdminPipelinesService({} as never);

function healthyCredits(): AdminEngineCreditHealth {
  return {
    countsByStatus: { pending: 0, processing: 0, retry: 0, confirmed: 0, dead: 0 },
    retryHighAttempts: 0,
    oldestUnconfirmedCreatedAt: null,
    oldestUnconfirmedAgeSeconds: null,
    stuckNano: "0",
  };
}

function healthyWebhooks(): AdminWebhookHealth {
  return { failedTotal: 0, failed24h: 0, recentFailures: [] };
}

function healthyEmail(): AdminEmailOutboxHealth {
  return { failedTotal: 0, recentFailures: [] };
}

function healthyPricingJobs(): AdminPricingJobHealth {
  return {
    countsByStatus: { pending: 0, processing: 0, retry: 0, confirmed: 0 },
    recentErrors: [],
    oldestUnconfirmedCreatedAt: null,
    oldestUnconfirmedAgeSeconds: null,
  };
}


function mockAll(overrides: {
  credits?: AdminEngineCreditHealth;
  webhooks?: AdminWebhookHealth;
  email?: AdminEmailOutboxHealth;
  pricingJobs?: AdminPricingJobHealth;
} = {}): void {
  creditsMock.mockResolvedValue(overrides.credits ?? healthyCredits());
  webhooksMock.mockResolvedValue(overrides.webhooks ?? healthyWebhooks());
  emailMock.mockResolvedValue(overrides.email ?? healthyEmail());
  pricingJobsMock.mockResolvedValue(overrides.pricingJobs ?? healthyPricingJobs());
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("admin pipeline health mapping", () => {
  it("maps every section with ISO dates and exact nano strings", async () => {
    const oldestCredit = new Date("2026-07-20T10:00:00Z");
    const receivedAt = new Date("2026-07-31T09:30:00Z");
    const oldestJob = new Date("2026-07-28T12:00:00Z");
    mockAll({
      credits: {
        countsByStatus: { pending: 2, processing: 1, retry: 0, confirmed: 41, dead: 0 },
        retryHighAttempts: 0,
        oldestUnconfirmedCreatedAt: oldestCredit,
        oldestUnconfirmedAgeSeconds: 950400,
        stuckNano: "25000000000", // $25
      },
      webhooks: {
        failedTotal: 3,
        failed24h: 0,
        recentFailures: [
          {
            provider: "cryptomus",
            eventType: "payment.paid",
            attempts: 5,
            receivedAt,
            lastError: "payment not found",
          },
        ],
      },
      email: {
        failedTotal: 1,
        recentFailures: [
          { template: "business_invite", attempts: 5, lastError: "SMTP 550 mailbox unavailable" },
        ],
      },
      pricingJobs: {
        countsByStatus: { pending: 1, processing: 0, retry: 0, confirmed: 17 },
        recentErrors: [
          {
            userId: "u1",
            engineAccountId: "acc-1",
            reason: "tier_change",
            status: "processing",
            attempts: 2,
            lastError: "engine 502",
          },
        ],
        oldestUnconfirmedCreatedAt: oldestJob,
        oldestUnconfirmedAgeSeconds: 259200,
      },
    });

    const value = await service.pipelineHealth() as Record<string, any>;

    expect(value.verdict).toBe("warn"); // email_outbox.failed_total = 1
    expect(value.engine_credits).toEqual({
      counts_by_status: { pending: 2, processing: 1, retry: 0, confirmed: 41, dead: 0 },
      dead_count: 0,
      retry_high_attempts_count: 0,
      oldest_unconfirmed_created_at: oldestCredit.toISOString(),
      oldest_unconfirmed_age_seconds: 950400,
      stuck_nano: "25000000000",
      stuck_usd: "25",
    });
    expect(value.webhook_events).toEqual({
      failed_total: 3,
      failed_24h: 0,
      recent_failures: [
        {
          provider: "cryptomus",
          event_type: "payment.paid",
          attempts: 5,
          received_at: receivedAt.toISOString(),
          last_error: "payment not found",
        },
      ],
    });
    // payload вебхуков и писем в ответе отсутствует by construction.
    expect(JSON.stringify(value.webhook_events)).not.toContain("payload");
    expect(value.email_outbox).toEqual({
      failed_total: 1,
      recent_failures: [
        { template: "business_invite", attempts: 5, last_error: "SMTP 550 mailbox unavailable" },
      ],
    });
    expect(value.engine_pricing_jobs).toEqual({
      counts_by_status: { pending: 1, processing: 0, retry: 0, confirmed: 17 },
      retry_count: 0,
      oldest_unconfirmed_created_at: oldestJob.toISOString(),
      oldest_unconfirmed_age_seconds: 259200,
      recent_errors: [
        {
          user_id: "u1",
          engine_account_id: "acc-1",
          reason: "tier_change",
          status: "processing",
          attempts: 2,
          last_error: "engine 502",
        },
      ],
    });
    // The retired release backfill lane no longer reports here: it does not exist.
    expect(value.pricing_backfill).toBeUndefined();
    expect(typeof value.generated_at).toBe("string");
  });

  it("returns null ages and zero sums instead of NaN for empty tables", async () => {
    mockAll();

    const value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("ok");
    expect(value.verdict_reasons).toEqual([]);
    expect(value.engine_credits).toMatchObject({
      counts_by_status: { pending: 0, processing: 0, retry: 0, confirmed: 0, dead: 0 },
      oldest_unconfirmed_created_at: null,
      oldest_unconfirmed_age_seconds: null,
      stuck_nano: "0",
      stuck_usd: "0",
    });
    expect(value.engine_pricing_jobs).toMatchObject({
      oldest_unconfirmed_created_at: null,
      oldest_unconfirmed_age_seconds: null,
    });
    expect(value.webhook_events.recent_failures).toEqual([]);
    expect(value.email_outbox.recent_failures).toEqual([]);
    expect(JSON.stringify(value)).not.toContain("NaN");
  });
});

describe("admin pipeline health verdict", () => {
  it("is bad when any engine credit is dead", async () => {
    mockAll({
      credits: {
        ...healthyCredits(),
        countsByStatus: { pending: 0, processing: 0, retry: 0, confirmed: 10, dead: 2 },
        stuckNano: "5000000000",
      },
    });
    const value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("bad");
    expect(value.verdict_reasons).toEqual(["engine_credits: 2 dead"]);
  });

  it("is bad on fresh failed webhooks even without dead credits", async () => {
    mockAll({
      webhooks: { ...healthyWebhooks(), failedTotal: 4, failed24h: 1 },
    });
    const value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("bad");
    expect(value.verdict_reasons).toEqual(["webhook_events: 1 failed in last 24h"]);
  });

  it("ignores stale failed webhooks older than 24h for the verdict", async () => {
    mockAll({
      webhooks: { ...healthyWebhooks(), failedTotal: 9, failed24h: 0 },
    });
    const value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("ok");
    expect(value.webhook_events.failed_total).toBe(9);
  });

  it("is warn on credit retry backlog, pricing job retry or failed email", async () => {
    mockAll({
      credits: {
        ...healthyCredits(),
        countsByStatus: { pending: 0, processing: 0, retry: 3, confirmed: 8, dead: 0 },
        retryHighAttempts: 2,
      },
    });
    let value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("warn");
    expect(value.verdict_reasons).toEqual(["engine_credits: 3 in retry"]);

    mockAll({
      pricingJobs: {
        ...healthyPricingJobs(),
        countsByStatus: { pending: 1, processing: 0, retry: 2, confirmed: 5 },
      },
    });
    value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("warn");
    expect(value.verdict_reasons).toEqual(["engine_pricing_jobs: 2 in retry"]);

    mockAll({ email: { ...healthyEmail(), failedTotal: 2 } });
    value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("warn");
    expect(value.verdict_reasons).toEqual(["email_outbox: 2 failed"]);
  });

  it("reports bad reasons first and keeps warn backlog alongside", async () => {
    mockAll({
      credits: {
        ...healthyCredits(),
        countsByStatus: { pending: 0, processing: 0, retry: 1, confirmed: 3, dead: 1 },
      },
      webhooks: { ...healthyWebhooks(), failedTotal: 2, failed24h: 2 },
      email: { ...healthyEmail(), failedTotal: 1 },
    });
    const value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("bad");
    expect(value.verdict_reasons).toEqual([
      "engine_credits: 1 dead",
      "webhook_events: 2 failed in last 24h",
      "engine_credits: 1 in retry",
      "email_outbox: 1 failed",
    ]);
  });

  it("is ok when queues are empty or fully confirmed", async () => {
    mockAll({
      credits: {
        ...healthyCredits(),
        countsByStatus: { pending: 0, processing: 0, retry: 0, confirmed: 100, dead: 0 },
      },
      pricingJobs: {
        ...healthyPricingJobs(),
        countsByStatus: { pending: 0, processing: 0, retry: 0, confirmed: 20 },
      },
    });
    const value = await service.pipelineHealth() as Record<string, any>;
    expect(value.verdict).toBe("ok");
    expect(value.verdict_reasons).toEqual([]);
  });
});
