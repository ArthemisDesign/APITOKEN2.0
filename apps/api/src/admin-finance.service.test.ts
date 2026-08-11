import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@claude-api/db", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@claude-api/db")>();
  return {
    ...actual,
    getAdminFinanceOverview: vi.fn(),
    getAdminFinanceFunnel: vi.fn(),
    listAdminEngineAccountOwners: vi.fn(),
    listAdminFinanceChurnSignals: vi.fn(),
    listAdminFinanceCohorts: vi.fn(),
    listAdminFinanceRevenueDaily: vi.fn(),
    listAdminFinanceTopCustomers: vi.fn(),
    listAdminPayingUsers: vi.fn(),
    listAdminRefunds: vi.fn(),
  };
});

import {
  getAdminFinanceOverview,
  getAdminFinanceFunnel,
  listAdminEngineAccountOwners,
  listAdminFinanceChurnSignals,
  listAdminFinanceCohorts,
  listAdminFinanceRevenueDaily,
  listAdminFinanceTopCustomers,
  listAdminPayingUsers,
  listAdminRefunds,
} from "@claude-api/db";
import type { EngineUsage } from "@claude-api/contracts";
import { AdminFinanceService } from "./admin-finance.service.js";

const overviewMock = vi.mocked(getAdminFinanceOverview);
const funnelMock = vi.mocked(getAdminFinanceFunnel);
const revenueDailyMock = vi.mocked(listAdminFinanceRevenueDaily);
const topCustomersMock = vi.mocked(listAdminFinanceTopCustomers);
const payingUsersMock = vi.mocked(listAdminPayingUsers);
const refundsMock = vi.mocked(listAdminRefunds);
const cohortsMock = vi.mocked(listAdminFinanceCohorts);
const churnMock = vi.mocked(listAdminFinanceChurnSignals);
const engineOwnersMock = vi.mocked(listAdminEngineAccountOwners);
const getSpendStats = vi.fn();
const getUsage = vi.fn();

// db-слой замокан на уровне функций-репозиториев, поэтому Database не используется.
const service = new AdminFinanceService({} as never, { getSpendStats, getUsage } as never);

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("admin finance overview", () => {
  it("derives delta, ARPU/ARPPU, paying share, average check and customer classes", async () => {
    overviewMock.mockResolvedValue({
      revenue30dNano: "150000000000", // $150
      revenuePrev30dNano: "100000000000", // $100 → +50%
      payments30dCount: 6,
      payingUsers30d: 3,
      activeUsers30d: 10,
      customerClasses: [
        { customerType: "b2b", users: 2 },
        { customerType: "b2c", users: 8 },
      ],
    });

    const value = await service.overview() as Record<string, unknown>;
    expect(value).toMatchObject({
      revenue_30d_nano: "150000000000",
      revenue_30d_usd: "150",
      revenue_prev_30d_nano: "100000000000",
      revenue_delta_pct: 50,
      payments_30d_count: 6,
      paying_users_30d: 3,
      active_users_30d: 10,
      arpu_30d_nano: "15000000000", // $150 / 10 активных
      arpu_30d_usd: "15",
      arppu_30d_nano: "50000000000", // $150 / 3 платящих
      arppu_30d_usd: "50",
      paying_share_pct: 30,
      avg_check_30d_nano: "25000000000", // $150 / 6 платежей
      avg_check_30d_usd: "25",
    });
    expect(value.customer_classes).toEqual([
      { customer_class: "b2b", users: 2 },
      { customer_class: "b2c", users: 8 },
    ]);
  });

  it("returns null ratios instead of NaN for empty periods", async () => {
    overviewMock.mockResolvedValue({
      revenue30dNano: "0",
      revenuePrev30dNano: "0",
      payments30dCount: 0,
      payingUsers30d: 0,
      activeUsers30d: 0,
      customerClasses: [],
    });

    const value = await service.overview() as Record<string, unknown>;
    expect(value).toMatchObject({
      revenue_30d_nano: "0",
      revenue_30d_usd: "0",
      revenue_delta_pct: null,
      arpu_30d_nano: null,
      arpu_30d_usd: null,
      arppu_30d_nano: null,
      paying_share_pct: null,
      avg_check_30d_nano: null,
    });
    expect(JSON.stringify(value)).not.toContain("NaN");
  });

  it("keeps a negative delta and reports null delta when the base period is zero", async () => {
    overviewMock.mockResolvedValue({
      revenue30dNano: "40000000000",
      revenuePrev30dNano: "50000000000",
      payments30dCount: 2,
      payingUsers30d: 1,
      activeUsers30d: 4,
      customerClasses: [],
    });
    await expect(service.overview()).resolves.toMatchObject({ revenue_delta_pct: -20 });

    overviewMock.mockResolvedValue({
      revenue30dNano: "40000000000",
      revenuePrev30dNano: "0",
      payments30dCount: 2,
      payingUsers30d: 1,
      activeUsers30d: 4,
      customerClasses: [],
    });
    await expect(service.overview()).resolves.toMatchObject({
      revenue_delta_pct: null,
      arpu_30d_nano: "10000000000",
      arppu_30d_nano: "40000000000",
      paying_share_pct: 25,
    });
  });
});

describe("admin finance revenue series", () => {
  it("pivots daily provider rows into per-day totals and grand totals", async () => {
    revenueDailyMock.mockResolvedValue([
      { day: "2026-07-29", provider: "cryptomus", totalNano: "10000000000", paymentsCount: 1 },
      { day: "2026-07-29", provider: "digiseller", totalNano: "5000000000", paymentsCount: 2 },
      { day: "2026-07-30", provider: "cryptomus", totalNano: "25000000000", paymentsCount: 1 },
    ]);

    const value = await service.revenue(30) as {
      days: number;
      series: Array<Record<string, unknown>>;
      totals: Record<string, unknown>;
    };
    expect(revenueDailyMock).toHaveBeenCalledWith(expect.anything(), 30);
    expect(value.series).toEqual([
      {
        day: "2026-07-29",
        total_nano: "15000000000",
        total_usd: "15",
        payments_count: 3,
        by_provider: { cryptomus: "10000000000", digiseller: "5000000000" },
      },
      {
        day: "2026-07-30",
        total_nano: "25000000000",
        total_usd: "25",
        payments_count: 1,
        by_provider: { cryptomus: "25000000000" },
      },
    ]);
    expect(value.totals).toEqual({
      total_nano: "40000000000",
      total_usd: "40",
      payments_count: 4,
      by_provider: { cryptomus: "35000000000", digiseller: "5000000000" },
    });
  });

  it("returns an empty series with zero totals for a silent period", async () => {
    revenueDailyMock.mockResolvedValue([]);
    const value = await service.revenue(7) as { series: unknown[]; totals: Record<string, unknown> };
    expect(value.series).toEqual([]);
    expect(value.totals).toEqual({
      total_nano: "0",
      total_usd: "0",
      payments_count: 0,
      by_provider: {},
    });
  });
});

describe("admin finance funnel", () => {
  it("computes per-provider and weighted total conversion, timing and average check", async () => {
    funnelMock.mockResolvedValue([
      {
        provider: "cryptomus",
        created: 10, paid: 5, canceled: 2, failed: 1, expired: 1, pending: 1,
        avgSecondsToPay: 600, paidTimed: 4, paidNano: "100000000000",
      },
      {
        provider: "digiseller",
        created: 10, paid: 1, canceled: 1, failed: 2, expired: 3, pending: 3,
        avgSecondsToPay: 1800, paidTimed: 1, paidNano: "10000000000",
      },
    ]);

    const value = await service.funnel(30) as {
      totals: Record<string, unknown>;
      by_provider: Array<Record<string, unknown>>;
    };
    expect(value.totals).toMatchObject({
      created: 20,
      paid: 6,
      canceled: 3,
      failed: 3,
      expired: 4,
      pending: 4,
      conversion_pct: 30,
      avg_seconds_to_pay: 840, // (600*4 + 1800*1) / 5
      avg_check_nano: "18333333333", // $110 / 6
      paid_nano: "110000000000",
      paid_usd: "110",
    });
    expect(value.by_provider[0]).toMatchObject({
      provider: "cryptomus",
      conversion_pct: 50,
      avg_seconds_to_pay: 600,
      avg_check_nano: "20000000000",
    });
    expect(value.by_provider[1]).toMatchObject({
      provider: "digiseller",
      conversion_pct: 10,
      avg_check_usd: "10",
    });
  });

  it("returns null conversion and timing for an empty window", async () => {
    funnelMock.mockResolvedValue([]);
    const value = await service.funnel(30) as { totals: Record<string, unknown> };
    expect(value.totals).toMatchObject({
      created: 0,
      paid: 0,
      conversion_pct: null,
      avg_seconds_to_pay: null,
      avg_check_nano: null,
      paid_nano: "0",
    });
    expect(JSON.stringify(value)).not.toContain("NaN");
  });
});

describe("admin finance top customers", () => {
  it("computes shares of the window totals for both lists", async () => {
    topCustomersMock.mockResolvedValue({
      topups: [
        { userId: "u1", email: "a@example.com", totalNano: "75000000000", paymentsCount: 3 },
        { userId: "u2", email: "b@example.com", totalNano: "25000000000", paymentsCount: 1 },
      ],
      topupsTotalNano: "200000000000",
      spend: [
        { userId: "u1", email: "a@example.com", spentNano: "40000000000" },
      ],
      spendTotalNano: "80000000000",
    });

    const value = await service.topCustomers(30, 20) as {
      topups: Array<Record<string, unknown>>;
      spend: Array<Record<string, unknown>>;
      totals: Record<string, unknown>;
    };
    expect(topCustomersMock).toHaveBeenCalledWith(expect.anything(), 30, 20);
    expect(value.topups[0]).toMatchObject({ share_pct: 37.5, total_usd: "75", payments_count: 3 });
    expect(value.topups[1]).toMatchObject({ share_pct: 12.5 });
    expect(value.spend[0]).toMatchObject({ share_pct: 50, spent_usd: "40" });
    expect(value.totals).toMatchObject({
      topups_nano: "200000000000",
      topups_usd: "200",
      spend_nano: "80000000000",
      spend_usd: "80",
    });
  });

  it("returns null shares when the window total is zero", async () => {
    topCustomersMock.mockResolvedValue({
      topups: [], topupsTotalNano: "0", spend: [], spendTotalNano: "0",
    });
    const value = await service.topCustomers(30, 20) as {
      topups: unknown[];
      totals: Record<string, unknown>;
    };
    expect(value.topups).toEqual([]);
    expect(value.totals).toMatchObject({ topups_nano: "0", spend_nano: "0" });
    expect(JSON.stringify(value)).not.toContain("NaN");
  });
});

describe("admin finance paying users", () => {
  const usage = (account: string, overrides: Partial<EngineUsage> = {}): EngineUsage => ({
    account,
    window: "7d",
    since_ts: 1,
    until_ts: 2,
    requests: 1,
    total_official_nano: "1",
    total_charged_nano: "1",
    buckets: {
      input: { tokens: 0, official_nano: "0" },
      output: { tokens: 0, official_nano: "0" },
      cache_read: { tokens: 0, official_nano: "0" },
      cache_write: { tokens: 0, official_nano: "0" },
      web_search: { requests: 0, official_nano: "0" },
      unattributed_legacy: { official_nano: "0" },
    },
    models: [],
    daily: [],
    daily_providers: [],
    keys: [],
    ...overrides,
  });

  it("keeps the default response DB-only and omits internal account fields", async () => {
    payingUsersMock.mockResolvedValue({
      days: 30, total: 1, limit: 50, offset: 0,
      summary: {
        payingUsers: 1, cohortUsers: 1, bonusOnlyUsers: 0, activeSpenders: 1,
        paidNano: "10", manualPaidNano: "0", spentNano: "1", bonusOnlySpentNano: "0",
        providerSpendNano: { anthropic: "1", openai: "0", google: "0", kimi: "0", other: "0" },
        providerUsers: { anthropic: 1, openai: 0, google: 0, kimi: 0, other: 0 },
      },
      rows: [{
        userId: "u1", email: "paid@example.com", displayName: "Paid", status: "active",
        customerType: "b2c", tier: 1, multiplierBp: 5000, fundingKind: "payments",
        paidNano: "10", paymentsCount: 1, manualPaidNano: "0", manualTopupsCount: 0,
        lastPaidAt: null, spentNano: "1", paidFundedSpentNano: "1",
        bonusFundedSpentNano: "0", otherFundedSpentNano: "0", unattributedSpentNano: "0",
        providerSpendNano: { anthropic: "1", openai: "0", google: "0", kimi: "0", other: "0" },
        engineAccountId: "acct_current", usageAccountIds: ["acct_old", "acct_current"],
        activeApiKeys: 1, lastSeenAt: null, createdAt: new Date("2026-06-01T10:00:00Z"),
      }],
    });

    const value = await service.payingUsers({ days: 30, funding: "all" }) as {
      rows: Array<Record<string, unknown>>;
    };

    expect(getUsage).not.toHaveBeenCalled();
    expect(value.rows[0]).not.toHaveProperty("usage");
    expect(value.rows[0]).not.toHaveProperty("engine_account_id");
    expect(value.rows[0]).not.toHaveProperty("usageAccountIds");
    expect(JSON.stringify(value)).not.toContain("acct_");
  });

  it("aggregates every historical account by exact provider and model without leaking EngineUsage", async () => {
    payingUsersMock.mockResolvedValue({
      days: 30,
      total: 1,
      limit: 50,
      offset: 0,
      summary: {
        payingUsers: 4,
        cohortUsers: 5,
        bonusOnlyUsers: 1,
        activeSpenders: 4,
        paidNano: "120000000000",
        manualPaidNano: "20000000000",
        spentNano: "123456789012345678901234567890",
        bonusOnlySpentNano: "99999999999999999999999999999",
        providerSpendNano: {
          anthropic: "12000000000", openai: "15000000000", google: "5000000000", kimi: "0", other: "0",
        },
        providerUsers: { anthropic: 2, openai: 3, google: 1, kimi: 0, other: 0 },
      },
      rows: [{
        userId: "u1",
        email: "paid@example.com",
        displayName: "Paid",
        status: "active",
        customerType: "b2c",
        tier: 1,
        multiplierBp: 5000,
        fundingKind: "bonus_only",
        paidNano: "0",
        paymentsCount: 0,
        manualPaidNano: "0",
        manualTopupsCount: 0,
        lastPaidAt: null,
        spentNano: "123456789012345678901234567890",
        paidFundedSpentNano: "0",
        bonusFundedSpentNano: "123456789012345678901234567890",
        otherFundedSpentNano: "0",
        unattributedSpentNano: "0",
        providerSpendNano: {
          anthropic: "2000000000", openai: "4000000000", google: "1000000000", kimi: "0", other: "0",
        },
        engineAccountId: "acct_u1",
        usageAccountIds: ["acct_u1", "acct_old_u1", "acct_u1"],
        activeApiKeys: 1,
        lastSeenAt: null,
        createdAt: new Date("2026-06-01T10:00:00Z"),
      }],
    });
    const exact = "900719925474099312345678901234567890";
    getUsage.mockImplementation(async (account: string) => account === "acct_u1"
      ? usage(account, {
        window: "30d",
        requests: 3,
        total_official_nano: exact,
        total_charged_nano: `${exact}1`,
        models: [
          {
            model: "future-model",
            provider: "free-community-provider",
            requests: 2,
            input_tokens: 11,
            output_tokens: 12,
            cache_read_tokens: 13,
            cache_write_5m_tokens: 14,
            cache_write_1h_tokens: 15,
            web_search_requests: 16,
            official_nano: exact,
            charged_nano: `${exact}1`,
          },
          {
            model: "future-model",
            requests: 1,
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_write_5m_tokens: 4,
            cache_write_1h_tokens: 5,
            web_search_requests: 6,
            official_nano: "7",
            charged_nano: "8",
          },
        ],
      })
      : usage(account, {
        window: "30d",
        requests: 4,
        total_official_nano: "10",
        total_charged_nano: "20",
        models: [{
          model: "future-model",
          provider: "free-community-provider",
          requests: 4,
          input_tokens: 21,
          output_tokens: 22,
          cache_read_tokens: 23,
          cache_write_5m_tokens: 24,
          cache_write_1h_tokens: 25,
          web_search_requests: 26,
          official_nano: "10",
          charged_nano: "20",
        }],
      }));

    const query = {
      days: 30 as const, limit: 50, sort: "spent" as const, dir: "desc" as const, includeUsage: true,
    };
    const value = await service.payingUsers(query) as {
      summary: Record<string, unknown>; rows: Array<Record<string, unknown>>;
    };
    expect(payingUsersMock).toHaveBeenCalledWith(expect.anything(), query);
    expect(value.summary).toMatchObject({
      paying_users: 4,
      cohort_users: 5,
      bonus_only_users: 1,
      paid_nano: "120000000000",
      manual_paid_nano: "20000000000",
      spent_nano: "123456789012345678901234567890",
      bonus_only_spent_nano: "99999999999999999999999999999",
      provider_spend: {
        anthropic_nano: "12000000000",
        openai_nano: "15000000000",
        google_nano: "5000000000",
        other_nano: "0",
      },
    });
    expect(value.rows[0]).toMatchObject({
      user_id: "u1",
      funding_kind: "bonus_only",
      last_paid_at: null,
      last_seen_at: null,
      created_at: "2026-06-01T10:00:00.000Z",
    });
    expect(value.rows[0]!.usage).toEqual({
      status: "complete",
      window: "30d",
      account_count: 2,
      available_account_count: 2,
      unavailable_account_count: 0,
      requests: "7",
      total_official_nano: (BigInt(exact) + 10n).toString(),
      total_charged_nano: (BigInt(`${exact}1`) + 20n).toString(),
      models: [
        {
          provider: null,
          model: "future-model",
          requests: "1",
          input_tokens: "1",
          output_tokens: "2",
          cache_write_5m_tokens: "4",
          cache_write_1h_tokens: "5",
          cache_read_tokens: "3",
          web_search_requests: "6",
          official_nano: "7",
          charged_nano: "8",
        },
        {
          provider: "free-community-provider",
          model: "future-model",
          requests: "6",
          input_tokens: "32",
          output_tokens: "34",
          cache_write_5m_tokens: "38",
          cache_write_1h_tokens: "40",
          cache_read_tokens: "36",
          web_search_requests: "42",
          official_nano: (BigInt(exact) + 10n).toString(),
          charged_nano: (BigInt(`${exact}1`) + 20n).toString(),
        },
      ],
    });
    expect(getUsage.mock.calls).toEqual([
      ["acct_u1", "30d", { signal: expect.any(AbortSignal) }],
      ["acct_old_u1", "30d", { signal: expect.any(AbortSignal) }],
    ]);
    const serialized = JSON.stringify(value.rows[0]!.usage);
    for (const forbidden of ["account", "since_ts", "until_ts", "buckets", "daily", "keys"]) {
      expect(serialized).not.toContain(`\"${forbidden}\"`);
    }
    expect(JSON.stringify(value)).not.toContain("_usd");
  });

  it("bounds all account calls globally and reports complete, partial, and unavailable coverage", async () => {
    const baseRow = {
      email: "spender@example.com", displayName: "Spender", status: "active" as const,
      customerType: "b2c" as const, tier: 0, multiplierBp: 5000,
      fundingKind: "spend_only" as const, paidNano: "0", paymentsCount: 0,
      manualPaidNano: "0", manualTopupsCount: 0, lastPaidAt: null,
      spentNano: "1", paidFundedSpentNano: "0", bonusFundedSpentNano: "0",
      otherFundedSpentNano: "0", unattributedSpentNano: "1",
      providerSpendNano: { anthropic: "0", openai: "0", google: "0", kimi: "0", other: "1" },
      activeApiKeys: 0, lastSeenAt: null, createdAt: new Date("2026-06-01T10:00:00Z"),
    };
    const rows = Array.from({ length: 6 }, (_, index) => ({
      ...baseRow,
      userId: `u${index}`,
      engineAccountId: index === 5 ? null : `acct_u${index}`,
      usageAccountIds: index === 0
        ? ["acct_u0", "acct_old_u0"]
        : index === 5 ? [] : [`acct_u${index}`],
    }));
    payingUsersMock.mockResolvedValue({
      days: 7, total: 200, limit: 6, offset: 100, rows,
      summary: {
        payingUsers: 0, cohortUsers: 6, bonusOnlyUsers: 0, activeSpenders: 6,
        paidNano: "0", manualPaidNano: "0", spentNano: "6", bonusOnlySpentNano: "0",
        providerSpendNano: { anthropic: "0", openai: "0", google: "0", kimi: "0", other: "6" },
        providerUsers: { anthropic: 0, openai: 0, google: 0, kimi: 0, other: 6 },
      },
    });

    let active = 0;
    let maxActive = 0;
    const releases: Array<() => void> = [];
    getUsage.mockImplementation((account: string, window: string) => new Promise((resolve, reject) => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      releases.push(() => {
        active -= 1;
        if (account === "acct_old_u0") reject(new Error("engine unavailable"));
        else resolve(usage(account, { window }));
      });
    }));

    const pending = service.payingUsers({
      days: 7, limit: 6, offset: 100, funding: "spenders", includeUsage: true,
    });
    await vi.waitFor(() => expect(getUsage).toHaveBeenCalledTimes(4));
    releases.splice(0).forEach((release) => release());
    await vi.waitFor(() => expect(getUsage).toHaveBeenCalledTimes(6));
    releases.splice(0).forEach((release) => release());
    const value = await pending as { rows: Array<Record<string, unknown>> };

    expect(maxActive).toBe(4);
    expect(getUsage.mock.calls.map(([account, window]) => [account, window])).toEqual([
      ["acct_u0", "7d"], ["acct_old_u0", "7d"], ["acct_u1", "7d"],
      ["acct_u2", "7d"], ["acct_u3", "7d"], ["acct_u4", "7d"],
    ]);
    expect(value.rows[0]?.usage).toEqual({
      status: "partial", window: "7d", account_count: 2,
      available_account_count: 1, unavailable_account_count: 1,
      requests: "1", total_official_nano: "1", total_charged_nano: "1", models: [],
    });
    expect(value.rows[1]?.usage).toEqual({
      status: "complete", window: "7d", account_count: 1,
      available_account_count: 1, unavailable_account_count: 0,
      requests: "1", total_official_nano: "1", total_charged_nano: "1", models: [],
    });
    expect(value.rows[5]).toMatchObject({
      usage: {
        status: "unavailable", window: "7d", account_count: 0,
        available_account_count: 0, unavailable_account_count: 0,
        requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [],
      },
    });
  });

  it("aborts in-flight usage and starts no queued calls after the page deadline", async () => {
    vi.useFakeTimers();
    const baseRow = {
      email: "spender@example.com", displayName: "Spender", status: "active" as const,
      customerType: "b2c" as const, tier: 0, multiplierBp: 5000,
      fundingKind: "spend_only" as const, paidNano: "0", paymentsCount: 0,
      manualPaidNano: "0", manualTopupsCount: 0, lastPaidAt: null,
      spentNano: "1", paidFundedSpentNano: "0", bonusFundedSpentNano: "0",
      otherFundedSpentNano: "0", unattributedSpentNano: "1",
      providerSpendNano: { anthropic: "0", openai: "0", google: "0", kimi: "0", other: "1" },
      activeApiKeys: 0, lastSeenAt: null, createdAt: new Date("2026-06-01T10:00:00Z"),
    };
    payingUsersMock.mockResolvedValue({
      days: 7, total: 6, limit: 6, offset: 0,
      rows: Array.from({ length: 6 }, (_, index) => ({
        ...baseRow, userId: `u${index}`, engineAccountId: `acct_u${index}`,
        usageAccountIds: [`acct_u${index}`],
      })),
      summary: {
        payingUsers: 0, cohortUsers: 6, bonusOnlyUsers: 0, activeSpenders: 6,
        paidNano: "0", manualPaidNano: "0", spentNano: "6", bonusOnlySpentNano: "0",
        providerSpendNano: { anthropic: "0", openai: "0", google: "0", kimi: "0", other: "6" },
        providerUsers: { anthropic: 0, openai: 0, google: 0, kimi: 0, other: 6 },
      },
    });
    getUsage.mockImplementation((_account: string, _window: string, options: { signal: AbortSignal }) =>
      new Promise((_resolve, reject) => {
        options.signal.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
      }));

    const pending = service.payingUsers({ days: 7, includeUsage: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(getUsage).toHaveBeenCalledTimes(4);
    await vi.advanceTimersByTimeAsync(5_000);
    const value = await pending as { rows: Array<Record<string, unknown>> };

    expect(getUsage).toHaveBeenCalledTimes(4);
    for (const row of value.rows) {
      expect(row.usage).toEqual({
        status: "unavailable", window: "7d", account_count: 1,
        available_account_count: 0, unavailable_account_count: 1,
        requests: "0", total_official_nano: "0", total_charged_nano: "0", models: [],
      });
    }
  });
});

describe("admin refunds", () => {
  it("maps rows and sums the page while keeping the grand total", async () => {
    const paidAt = new Date("2026-07-01T10:00:00Z");
    const updatedAt = new Date("2026-07-05T12:00:00Z");
    refundsMock.mockResolvedValue({
      rows: [
        {
          id: "p1", userId: "u1", email: "a@example.com", provider: "cryptomus",
          providerPaymentId: "prov-1", amountNano: "25000000000", currency: "USD",
          status: "refunded", adjustmentStatus: "confirmed", adjustmentConfirmedAt: updatedAt,
          adjustmentLastError: null, paidAt, updatedAt,
        },
        {
          id: "p2", userId: "u2", email: "b@example.com", provider: "digiseller",
          providerPaymentId: "prov-2", amountNano: "5000000000", currency: "USD",
          status: "disputed", adjustmentStatus: "retry", adjustmentConfirmedAt: null,
          adjustmentLastError: "engine unavailable", paidAt: null, updatedAt,
        },
      ],
      total: 7,
      totalNano: "90000000000",
    });

    const value = await service.refunds(50, 0) as {
      rows: Array<Record<string, unknown>>;
      total: number;
      limit: number;
      offset: number;
      page_amount_nano: string;
      total_amount_nano: string;
    };
    expect(refundsMock).toHaveBeenCalledWith(expect.anything(), 50, 0);
    expect(value.rows).toHaveLength(2);
    expect(value.rows[0]).toMatchObject({
      id: "p1",
      email: "a@example.com",
      amount_nano: "25000000000",
      amount_usd: "25",
      status: "refunded",
      adjustment_status: "confirmed",
      adjustment_confirmed_at: updatedAt.toISOString(),
      adjustment_last_error: null,
      paid_at: paidAt.toISOString(),
      updated_at: updatedAt.toISOString(),
    });
    expect(value.rows[1]).toMatchObject({
      status: "disputed",
      adjustment_status: "retry",
      adjustment_confirmed_at: null,
      adjustment_last_error: "engine unavailable",
      paid_at: null,
    });
    expect(value).toMatchObject({
      total: 7,
      page_amount_nano: "30000000000",
      page_amount_usd: "30",
      total_amount_nano: "90000000000",
      total_amount_usd: "90",
    });
  });

  it("returns a zero page sum for an empty page", async () => {
    refundsMock.mockResolvedValue({ rows: [], total: 0, totalNano: "0" });
    const value = await service.refunds(50, 100) as Record<string, unknown>;
    expect(value).toMatchObject({
      rows: [],
      total: 0,
      offset: 100,
      page_amount_nano: "0",
      page_amount_usd: "0",
    });
  });
});

describe("admin finance cohorts", () => {
  it("rounds the median and derives the paying share per cohort", async () => {
    cohortsMock.mockResolvedValue([
      { week: "2026-07-06", registered: 10, paidUsers: 4, medianDaysToFirstPayment: 2.345, revenueNano: "80000000000" },
      { week: "2026-07-13", registered: 5, paidUsers: 0, medianDaysToFirstPayment: null, revenueNano: "0" },
    ]);

    const value = await service.cohorts(8) as { cohorts: Array<Record<string, unknown>> };
    expect(cohortsMock).toHaveBeenCalledWith(expect.anything(), 8);
    expect(value.cohorts[0]).toMatchObject({
      week: "2026-07-06",
      registered: 10,
      paid_users: 4,
      paid_share_pct: 40,
      median_days_to_first_payment: 2.3,
      revenue_nano: "80000000000",
      revenue_usd: "80",
    });
    expect(value.cohorts[1]).toMatchObject({
      paid_users: 0,
      paid_share_pct: 0,
      median_days_to_first_payment: null,
      revenue_nano: "0",
    });
  });
});

describe("admin finance churn signals", () => {
  it("maps rows with nullable dates and exact money strings", async () => {
    churnMock.mockResolvedValue([
      {
        userId: "u1", email: "a@example.com",
        lastSeenAt: new Date("2026-07-10T08:00:00Z"),
        lastPaidAt: new Date("2026-06-28T08:00:00Z"),
        spent30dNano: "12345000000",
      },
      { userId: "u2", email: "b@example.com", lastSeenAt: null, lastPaidAt: null, spent30dNano: "0" },
    ]);

    const value = await service.churnSignals(14, 50) as { rows: Array<Record<string, unknown>> };
    expect(churnMock).toHaveBeenCalledWith(expect.anything(), 14, 50);
    expect(value.rows[0]).toMatchObject({
      user_id: "u1",
      last_seen_at: "2026-07-10T08:00:00.000Z",
      last_paid_at: "2026-06-28T08:00:00.000Z",
      spent_30d_nano: "12345000000",
      spent_30d_usd: "12.345",
    });
    expect(value.rows[1]).toMatchObject({ last_seen_at: null, last_paid_at: null, spent_30d_usd: "0" });
  });
});

describe("admin engine spend", () => {
  const period = (accounts: Array<Record<string, unknown>>) => ({
    requests: 10, charge_usd: 100, real_usd: 250,
    providers: [{ provider: "openai", requests: 6, charge_usd: 60, real_usd: 150 }],
    models: [{ model: "gpt-5.6-sol", provider: "openai", requests: 6, charge_usd: 60, real_usd: 150 }],
    accounts,
  });

  it("labels client accounts and separates engine-only spend", async () => {
    getSpendStats.mockResolvedValue({
      now: 1_800_000_000,
      periods: {
        d1: period([
          { account: "acct_client", handle: "user:u1", requests: 4, charge_usd: 40, real_usd: 100, last_ts: 1 },
          { account: "acct_ok", handle: "openkeys-abc", requests: 3, charge_usd: 35, real_usd: 90, last_ts: 2 },
          { account: "acct_int", handle: "crm-parsing", requests: 3, charge_usd: 25, real_usd: 60, last_ts: 3 },
        ]),
        d7: period([]),
        d30: period([]),
      },
    });
    engineOwnersMock.mockResolvedValue([
      {
        engineAccountId: "acct_client", userId: "u1", email: "a@example.com",
        displayName: "A", status: "active", customerType: "b2b",
      },
    ]);

    const value = await service.engineSpend(1) as {
      days: number;
      accounts: Array<Record<string, unknown>>;
      by_class: Record<string, Record<string, number>>;
      models: Array<Record<string, unknown>>;
    };

    expect(value.days).toBe(1);
    expect(value.models[0]).toMatchObject({ model: "gpt-5.6-sol", charge_usd: 60 });
    expect(value.accounts[0]).toMatchObject({
      account_class: "client",
      owner: { email: "a@example.com", customer_type: "b2b" },
    });
    expect(value.accounts[1]).toMatchObject({ account_class: "openkeys", owner: null });
    expect(value.accounts[2]).toMatchObject({ account_class: "internal", owner: null });
    expect(value.by_class.client).toMatchObject({ accounts: 1, charge_usd: 40 });
    expect(value.by_class.openkeys).toMatchObject({ accounts: 1, charge_usd: 35 });
    expect(value.by_class.internal).toMatchObject({ accounts: 1, charge_usd: 25 });
  });

  it("selects the requested window", async () => {
    getSpendStats.mockResolvedValue({
      now: 1,
      periods: { d1: period([]), d7: period([]), d30: { ...period([]), charge_usd: 999 } },
    });
    engineOwnersMock.mockResolvedValue([]);
    const value = await service.engineSpend(30) as { charge_usd: number };
    expect(value.charge_usd).toBe(999);
  });
});
