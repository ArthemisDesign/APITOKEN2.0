import { beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

// Общий с партнёрским порталом контракт формы строки. Второй конец — apps/sales-api/src/
// sync-feed.test.ts, который парсит этот же файл своей схемой. См. _comment внутри golden.
const GOLDEN = JSON.parse(readFileSync(
  new URL("../../../tests/contracts/sales-usage-feed.golden.json", import.meta.url),
  "utf8",
)) as { row: Record<string, unknown>; nextCursor: string };

const dbMocks = vi.hoisted(() => ({
  listUsageEventsAfter: vi.fn(),
}));

vi.mock("@claude-api/db", () => ({
  listUsageEventsAfter: dbMocks.listUsageEventsAfter,
  listPaidTopupsAfter: vi.fn(),
  listReferralAttributionsAfter: vi.fn(),
  listReferralProfiles: vi.fn(),
  setReferralFloor: vi.fn(),
}));

import { SalesFeedController } from "./sales-feed.controller.js";

describe("sales usage feed controller", () => {
  beforeEach(() => {
    dbMocks.listUsageEventsAfter.mockReset();
  });

  it("serializes the free-first commission basis and bigint money without loss", async () => {
    const occurredAt = new Date("2026-08-01T13:00:00.000Z");
    dbMocks.listUsageEventsAfter.mockResolvedValue({
      items: [{
        id: 44n,
        userId: "00000000-0000-4000-8000-000000000044",
        amountNano: 9_007_199_254_740_993n,
        providerId: "anthropic",
        accountClass: null,
        pricingMode: null,
        paidFundedNano: null,
        commissionEligible: null,
        snapshotDigest: null,
        occurredAt,
      }],
      nextCursor: 45n,
    });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.usageEvents("40", "10")).resolves.toEqual({
      items: [GOLDEN.row],
      nextCursor: GOLDEN.nextCursor,
    });
    expect(dbMocks.listUsageEventsAfter).toHaveBeenCalledWith({}, 40n, 10);
  });

  it("defaults malformed tokens and never sends an out-of-range bigint to PostgreSQL", async () => {
    dbMocks.listUsageEventsAfter.mockResolvedValue({ items: [], nextCursor: 0n });
    const controller = new SalesFeedController({} as never, {} as never);

    await controller.usageEvents("9223372036854775808", "1junk");
    expect(dbMocks.listUsageEventsAfter).toHaveBeenLastCalledWith({}, 0n, 1000);

    await controller.usageEvents("9223372036854775807", "99999999999999999999");
    expect(dbMocks.listUsageEventsAfter).toHaveBeenLastCalledWith({}, 9_223_372_036_854_775_807n, 2000);
  });

});
