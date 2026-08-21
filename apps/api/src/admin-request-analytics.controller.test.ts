import { BadRequestException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import { AdminRequestAnalyticsController } from "./admin-request-analytics.controller.js";
import type { AdminRequestAnalyticsService } from "./admin-request-analytics.service.js";

function setup() {
  const service = {
    summary: vi.fn(async (value) => value),
    page: vi.fn(async (value) => value),
    logical: vi.fn(async (value) => value),
  } as unknown as AdminRequestAnalyticsService;
  return { controller: new AdminRequestAnalyticsController(service), service };
}

describe("AdminRequestAnalyticsController", () => {
  it("forwards bounded summary and keyset queries", async () => {
    const { controller, service } = setup();
    await controller.summary("1", "2", "acct_1");
    await controller.page("1", "2", undefined, "cursor", "25");
    expect(service.summary).toHaveBeenCalledWith({ from: 1, to: 2, accountId: "acct_1" });
    expect(service.page).toHaveBeenCalledWith({ from: 1, to: 2, cursor: "cursor", limit: 25 });
  });

  it("rejects missing, reversed, oversized and over-limit queries", async () => {
    const { controller } = setup();
    expect(() => controller.summary(undefined, "2")).toThrow(BadRequestException);
    expect(() => controller.summary("2", "1")).toThrow(BadRequestException);
    expect(() => controller.summary("0", String(30 * 86_400 + 1))).toThrow(BadRequestException);
    expect(() => controller.page("1", "2", undefined, undefined, "201")).toThrow(BadRequestException);
    expect(() => controller.logical("not-a-uuid")).toThrow(BadRequestException);
  });
});
