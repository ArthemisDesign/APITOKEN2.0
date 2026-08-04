import { BadRequestException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import { AdminFinanceController } from "./admin-finance.controller.js";
import type { AdminFinanceService } from "./admin-finance.service.js";

describe("admin finance HTTP contract", () => {
  it("rejects malformed windows, limits and offsets before service calls", () => {
    const finance = fakeFinance();
    const controller = new AdminFinanceController(finance.service);

    expect(() => controller.revenue("15")).toThrow(BadRequestException);
    expect(() => controller.revenue("7.5")).toThrow(BadRequestException);
    expect(() => controller.funnel("0")).toThrow(BadRequestException);
    expect(() => controller.topCustomers("30", "0")).toThrow(BadRequestException);
    expect(() => controller.topCustomers("30", "101")).toThrow(BadRequestException);
    expect(() => controller.payingUsers("14")).toThrow(BadRequestException);
    expect(() => controller.payingUsers("30", "101")).toThrow(BadRequestException);
    expect(() => controller.payingUsers("30", "50", "0", "", undefined, "claude")).toThrow(BadRequestException);
    expect(() => controller.payingUsers("30", "50", "0", "", undefined, undefined, "raw_sql")).toThrow(BadRequestException);
    expect(() => controller.cohorts("0")).toThrow(BadRequestException);
    expect(() => controller.engineSpend("14")).toThrow(BadRequestException);
    expect(() => controller.churnSignals("91", undefined)).toThrow(BadRequestException);
    expect(() => controller.refunds("501", "0")).toThrow(BadRequestException);
    expect(() => controller.refunds("50", "-1")).toThrow(BadRequestException);
    expect(finance.revenue).not.toHaveBeenCalled();
    expect(finance.refunds).not.toHaveBeenCalled();
  });

  it("applies documented defaults and forwards validated parameters", async () => {
    const finance = fakeFinance();
    const controller = new AdminFinanceController(finance.service);
    finance.revenue.mockResolvedValue({ series: [] });
    finance.refunds.mockResolvedValue({ rows: [] });
    finance.topCustomers.mockResolvedValue({ topups: [] });
    finance.payingUsers.mockResolvedValue({ rows: [] });
    finance.churnSignals.mockResolvedValue({ rows: [] });
    finance.cohorts.mockResolvedValue({ cohorts: [] });
    finance.engineSpend.mockResolvedValue({ models: [] });

    await expect(controller.revenue(undefined)).resolves.toEqual({ series: [] });
    expect(finance.revenue).toHaveBeenCalledWith(30);
    await controller.revenue("90");
    expect(finance.revenue).toHaveBeenCalledWith(90);

    await controller.funnel(undefined);
    expect(finance.funnel).toHaveBeenCalledWith(30);

    await controller.topCustomers("7", "5");
    expect(finance.topCustomers).toHaveBeenCalledWith(7, 5);
    await controller.topCustomers(undefined, undefined);
    expect(finance.topCustomers).toHaveBeenCalledWith(30, 20);

    await controller.payingUsers(undefined, undefined, undefined, undefined, undefined, undefined, undefined, undefined);
    expect(finance.payingUsers).toHaveBeenCalledWith({
      days: 30, limit: 50, offset: 0, sort: "spent", dir: "desc",
    });
    await controller.payingUsers("7", "25", "50", "paid@", "active", "openai", "paid", "asc");
    expect(finance.payingUsers).toHaveBeenCalledWith({
      days: 7, limit: 25, offset: 50, q: "paid@", status: "active", provider: "openai",
      sort: "paid", dir: "asc",
    });

    await controller.cohorts(undefined);
    expect(finance.cohorts).toHaveBeenCalledWith(8);

    await controller.engineSpend(undefined);
    expect(finance.engineSpend).toHaveBeenCalledWith(30);
    await controller.engineSpend("1");
    expect(finance.engineSpend).toHaveBeenCalledWith(1);

    await controller.churnSignals(undefined, undefined);
    expect(finance.churnSignals).toHaveBeenCalledWith(14, 50);

    await controller.refunds(undefined, undefined);
    expect(finance.refunds).toHaveBeenCalledWith(50, 0);
    await controller.refunds("25", "50");
    expect(finance.refunds).toHaveBeenCalledWith(25, 50);
  });

  it("passes the overview through unchanged", async () => {
    const finance = fakeFinance();
    const controller = new AdminFinanceController(finance.service);
    finance.overview.mockResolvedValue({ revenue_30d_usd: "150" });
    await expect(controller.overview()).resolves.toEqual({ revenue_30d_usd: "150" });
    expect(finance.overview).toHaveBeenCalledTimes(1);
  });
});

function fakeFinance() {
  const finance = {
    overview: vi.fn(),
    revenue: vi.fn(),
    funnel: vi.fn(),
    topCustomers: vi.fn(),
    payingUsers: vi.fn(),
    refunds: vi.fn(),
    cohorts: vi.fn(),
    churnSignals: vi.fn(),
    engineSpend: vi.fn(),
  };
  return { ...finance, service: finance as unknown as AdminFinanceService };
}
