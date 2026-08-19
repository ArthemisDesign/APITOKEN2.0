import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AdminUserOverviewRow } from "@claude-api/db";
import { AdminOperationsService } from "./admin-operations.service.js";
import { AdminService } from "./admin.service.js";

// Unit-тесты HTTP-сервисного слоя админских списков: @claude-api/db замокан, проверяем
// проброс sort/dir и фильтров в db-функции, конверсию ответов (totals, envelope) и
// обратную совместимость дефолтной выдачи. SQL-уровень покрыт admin-overview.unit.test.ts
// в packages/db, HTTP-парсинг — *.controller.test.ts.

const dbMocks = vi.hoisted(() => ({
  listAdminUserOverview: vi.fn(),
  listAdminTopups: vi.fn(),
  listAdminAudit: vi.fn(),
  listAdminAuditActions: vi.fn(),
}));

vi.mock("@claude-api/db", async (importActual) => ({
  ...await importActual<Record<string, unknown>>(),
  listAdminUserOverview: dbMocks.listAdminUserOverview,
  listAdminTopups: dbMocks.listAdminTopups,
  listAdminAudit: dbMocks.listAdminAudit,
  listAdminAuditActions: dbMocks.listAdminAuditActions,
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe("AdminService.listUsers sorting", () => {
  it("forwards whitelisted sort/dir to the DB query", async () => {
    dbMocks.listAdminUserOverview.mockResolvedValue({ rows: [], total: 0, limit: 50, offset: 0 });
    const service = adminService();

    await expect(service.listUsers({ limit: 50, offset: 0, sort: "paid_total", dir: "asc" }))
      .resolves.toMatchObject({ total: 0, users: [] });
    expect(dbMocks.listAdminUserOverview).toHaveBeenCalledWith(
      expect.anything(),
      { limit: 50, offset: 0, sort: "paid_total", dir: "asc" },
    );
  });

  it("keeps the default query object filter-free (backward compatibility)", async () => {
    dbMocks.listAdminUserOverview.mockResolvedValue({ rows: [fakeOverviewRow()], total: 1, limit: 50, offset: 0 });
    const service = adminService();

    const page = await service.listUsers({ limit: 50, offset: 0 });
    expect(dbMocks.listAdminUserOverview).toHaveBeenCalledWith(expect.anything(), { limit: 50, offset: 0 });
    expect(page.users[0]).toMatchObject({
      email: "a@example.com",
      balance_usd: null,
      spent_usd: null,
      spent_30d_usd: "3.0000",
      provider_spend_30d: {
        anthropic_nano: "2000000000",
        openai_nano: "1000000000",
        google_nano: "0",
        kimi_nano: "0",
        other_nano: "0",
      },
      payments: { paid_count: 1, paid_total_usd: "25.0000" },
    });
  });
});

describe("AdminOperationsService.topups filters", () => {
  it("maps filtered rows and exposes per-list totals", async () => {
    dbMocks.listAdminTopups.mockResolvedValue({
      payments: [{
        id: "p1", userId: "u1", email: "a@example.com", provider: "cryptomus",
        providerPaymentId: "tx-1", amountNano: "25000000000", currency: "USD", status: "failed",
        paidAt: null, createdAt: new Date("2026-07-10T00:00:00.000Z"), creditStatus: null,
      }],
      checkouts: [],
      paymentsTotal: 7,
      checkoutsTotal: 3,
    });
    const service = new AdminOperationsService({} as never, {} as never);

    const result = await service.topups({
      limit: 10, offset: 30, q: "a@example", provider: "cryptomus", status: "failed",
    }) as { payments: Array<Record<string, unknown>>; payments_total: number; checkouts_total: number };

    expect(dbMocks.listAdminTopups).toHaveBeenCalledWith(expect.anything(), {
      limit: 10, offset: 30, q: "a@example", provider: "cryptomus", status: "failed",
    });
    expect(result.payments_total).toBe(7);
    expect(result.checkouts_total).toBe(3);
    // failed-платёж без оплаты: paid_at сериализуется как null, а не падает.
    expect(result.payments[0]).toMatchObject({ amount_usd: "25", status: "failed", paid_at: null });
  });

  it("keeps the default two-list response shape plus additive totals", async () => {
    dbMocks.listAdminTopups.mockResolvedValue({
      payments: [],
      checkouts: [{
        id: "c1", userId: "u1", email: "a@example.com", provider: "platega",
        providerPaymentId: null, amountUsd: "25", status: "pending",
        createdAt: new Date("2026-07-11T00:00:00.000Z"), completedAt: null, expiresAt: null,
      }],
      paymentsTotal: 0,
      checkoutsTotal: 1,
    });
    const service = new AdminOperationsService({} as never, {} as never);

    const result = await service.topups({ limit: 100, offset: 0 }) as Record<string, unknown>;
    expect(dbMocks.listAdminTopups).toHaveBeenCalledWith(expect.anything(), { limit: 100, offset: 0 });
    expect(Object.keys(result)).toEqual(["payments", "checkouts", "payments_total", "checkouts_total"]);
  });
});

describe("AdminOperationsService.audit filters", () => {
  it("returns the paged envelope with filters forwarded", async () => {
    dbMocks.listAdminAudit.mockResolvedValue({
      rows: [{
        id: "9", actorType: "commercial-admin", actorId: "admin-q", action: "admin.credit",
        targetType: "user", targetId: "u1", metadata: { ref: "r1" },
        createdAt: new Date("2026-07-15T12:00:00.000Z"),
      }],
      total: 42,
    });
    const service = new AdminOperationsService({} as never, {} as never);
    const from = new Date("2026-07-01T00:00:00.000Z");

    const result = await service.audit({ limit: 50, offset: 10, action: "admin.credit", from });

    expect(dbMocks.listAdminAudit).toHaveBeenCalledWith(expect.anything(), {
      limit: 50, offset: 10, action: "admin.credit", from,
    });
    expect(result).toEqual({
      rows: [{
        id: "9",
        actor_type: "commercial-admin",
        actor_id: "admin-q",
        action: "admin.credit",
        target_type: "user",
        target_id: "u1",
        metadata: { ref: "r1" },
        created_at: "2026-07-15T12:00:00.000Z",
      }],
      total: 42,
      limit: 50,
      offset: 10,
    });
  });

  it("exposes the distinct action list for the panel filter dropdown", async () => {
    dbMocks.listAdminAuditActions.mockResolvedValue(["admin.credit", "auth.login"]);
    const service = new AdminOperationsService({} as never, {} as never);

    await expect(service.auditActions()).resolves.toEqual({ actions: ["admin.credit", "auth.login"] });
    expect(dbMocks.listAdminAuditActions).toHaveBeenCalledWith(expect.anything());
  });
});

function adminService(): AdminService {
  const engine = { getAccounts: vi.fn().mockResolvedValue([]) };
  return new AdminService({} as never, engine as never, {} as never);
}

function fakeOverviewRow(): AdminUserOverviewRow {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    email: "a@example.com",
    displayName: "Alice",
    emailVerified: true,
    status: "active",
    createdAt: new Date("2026-01-01T00:00:00.000Z"),
    hasPassword: true,
    totpEnabled: false,
    providers: [],
    customerType: "b2c",
    currentTier: 0,
    multiplierBp: 4000,
    cumulativeTopupNano: "25000000000",
    tierWindowSpentNano: "0",
    engineAccountId: null,
    engineAccountStatus: null,
    pricingSyncStatus: null,
    pricingSyncAttempts: null,
    pricingSyncError: null,
    pricingSyncConfirmedAt: null,
    paidPaymentsCount: 1,
    paidTotalNano: "25000000000",
    lastPaidAt: new Date("2026-07-01T00:00:00.000Z"),
    pendingCheckoutsCount: 0,
    apiKeysActive: 1,
    apiKeysTotal: 1,
    lastSeenAt: new Date("2026-07-30T00:00:00.000Z"),
    spent30dNano: "3000000000",
    providerSpend30dNano: {
      anthropic: "2000000000",
      openai: "1000000000",
      google: "0",
      kimi: "0",
      other: "0",
    },
  };
}
