import { beforeEach, describe, expect, it, vi } from "vitest";

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

  it("serializes immutable attribution and bigint money without loss", async () => {
    const occurredAt = new Date("2026-08-01T13:00:00.000Z");
    dbMocks.listUsageEventsAfter.mockResolvedValue({
      items: [{
        id: 44n,
        userId: "00000000-0000-4000-8000-000000000044",
        amountNano: 9_007_199_254_740_993n,
        providerId: "anthropic",
        accountClass: "b2c",
        pricingMode: "track",
        paidFundedNano: 9_007_199_254_740_993n,
        commissionEligible: true,
        snapshotDigest: "snapshot-44",
        occurredAt,
      }],
      nextCursor: 45n,
    });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.usageEvents("40", "10")).resolves.toEqual({
      items: [{
        id: "44",
        userId: "00000000-0000-4000-8000-000000000044",
        amountNano: "9007199254740993",
        providerId: "anthropic",
        accountClass: "b2c",
        pricingMode: "track",
        paidFundedNano: "9007199254740993",
        commissionEligible: true,
        snapshotDigest: "snapshot-44",
        occurredAt: "2026-08-01T13:00:00.000Z",
      }],
      nextCursor: "45",
    });
    expect(dbMocks.listUsageEventsAfter).toHaveBeenCalledWith({}, 40n, 10);
  });

  it("keeps the all-null legacy shape explicit during rolling compatibility", async () => {
    dbMocks.listUsageEventsAfter.mockResolvedValue({
      items: [{
        id: 1n,
        userId: "00000000-0000-4000-8000-000000000001",
        amountNano: 10n,
        providerId: null,
        accountClass: null,
        pricingMode: null,
        paidFundedNano: null,
        commissionEligible: null,
        snapshotDigest: null,
        occurredAt: new Date("2026-08-01T00:00:00.000Z"),
      }],
      nextCursor: 1n,
    });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.usageEvents()).resolves.toMatchObject({
      items: [{
        paidFundedNano: null,
        providerId: null,
        commissionEligible: null,
      }],
    });
  });
});
