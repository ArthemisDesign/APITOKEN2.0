import { beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

// Общий с партнёрским порталом контракт формы строки. Второй конец — apps/sales-api/src/
// sync-feed.test.ts, который парсит этот же файл своей схемой. См. _comment внутри golden.
const GOLDEN = JSON.parse(readFileSync(
  new URL("../../../tests/contracts/sales-usage-feed.golden.json", import.meta.url),
  "utf8",
)) as { row: Record<string, unknown>; nextCursor: string; sourceHead: string };
const TOPUPS_V2_GOLDEN = JSON.parse(readFileSync(
  new URL("../../../tests/contracts/sales-topups-v2-feed.golden.json", import.meta.url),
  "utf8",
)) as { row: Record<string, unknown>; nextCursor: string; sourceHead: string };
const REVERSALS_GOLDEN = JSON.parse(readFileSync(
  new URL("../../../tests/contracts/sales-payment-reversals-feed.golden.json", import.meta.url),
  "utf8",
)) as { row: Record<string, unknown>; nextCursor: string; sourceHead: string };

const dbMocks = vi.hoisted(() => ({
  listUsageEventsAfter: vi.fn(),
  listPaidTopupsV2After: vi.fn(),
  listPaymentReversalsAfter: vi.fn(),
  setReferralFloor: vi.fn(),
}));

vi.mock("@claude-api/db", () => ({
  listUsageEventsAfter: dbMocks.listUsageEventsAfter,
  listPaidTopupsV2After: dbMocks.listPaidTopupsV2After,
  listPaymentReversalsAfter: dbMocks.listPaymentReversalsAfter,
  listPaidTopupsAfter: vi.fn(),
  listReferralAttributionsAfter: vi.fn(),
  listReferralProfiles: vi.fn(),
  setReferralFloor: dbMocks.setReferralFloor,
}));

import { SalesFeedController } from "./sales-feed.controller.js";

describe("sales usage feed controller", () => {
  beforeEach(() => {
    dbMocks.listUsageEventsAfter.mockReset();
    dbMocks.listPaidTopupsV2After.mockReset();
    dbMocks.listPaymentReversalsAfter.mockReset();
    dbMocks.setReferralFloor.mockReset();
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
      sourceHead: 46n,
    });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.usageEvents("40", "10")).resolves.toEqual({
      items: [GOLDEN.row],
      nextCursor: GOLDEN.nextCursor,
      sourceHead: GOLDEN.sourceHead,
    });
    expect(dbMocks.listUsageEventsAfter).toHaveBeenCalledWith({}, 40n, 10);
  });

  it("defaults malformed tokens and never sends an out-of-range bigint to PostgreSQL", async () => {
    dbMocks.listUsageEventsAfter.mockResolvedValue({ items: [], nextCursor: 0n, sourceHead: 0n });
    const controller = new SalesFeedController({} as never, {} as never);

    await controller.usageEvents("9223372036854775808", "1junk");
    expect(dbMocks.listUsageEventsAfter).toHaveBeenLastCalledWith({}, 0n, 1000);

    await controller.usageEvents("9223372036854775807", "99999999999999999999");
    expect(dbMocks.listUsageEventsAfter).toHaveBeenLastCalledWith({}, 9_223_372_036_854_775_807n, 2000);
  });

  it("serializes the commit-ordered topups-v2 cursor and bigint money without loss", async () => {
    const paidAt = new Date("2026-08-11T12:34:56.789Z");
    dbMocks.listPaidTopupsV2After.mockResolvedValue({
      items: [{
        id: 12n,
        paymentId: "10000000-0000-4000-8000-000000000012",
        userId: "20000000-0000-4000-8000-000000000012",
        amountNano: 9_007_199_254_740_993n,
        paidAt,
      }],
      nextCursor: 13n,
      sourceHead: 14n,
    });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.topupsV2("10", "25")).resolves.toEqual({
      items: [TOPUPS_V2_GOLDEN.row],
      nextCursor: TOPUPS_V2_GOLDEN.nextCursor,
      sourceHead: TOPUPS_V2_GOLDEN.sourceHead,
    });
    expect(dbMocks.listPaidTopupsV2After).toHaveBeenCalledWith({}, 10n, 25);
  });

  it("marks the legacy referral writer as non-pricing", async () => {
    dbMocks.setReferralFloor.mockResolvedValue({ applied: true, multiplierBp: null });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.referralDiscount({
      userId: "11111111-1111-4111-8111-111111111111",
      floorBps: 9_500,
    })).resolves.toEqual({ applied: true, multiplierBp: null, pricingAffected: false });
  });

  it("serializes immutable payment reversals with exact bigint money", async () => {
    dbMocks.listPaymentReversalsAfter.mockResolvedValue({
      items: [{
        id: 77n,
        paymentId: "10000000-0000-4000-8000-000000000077",
        userId: "20000000-0000-4000-8000-000000000077",
        kind: "refund",
        amountNano: 9_007_199_254_740_993n,
        reversedAt: new Date("2026-08-12T02:03:04.567Z"),
      }],
      nextCursor: 78n,
      sourceHead: 79n,
    });
    const controller = new SalesFeedController({} as never, {} as never);

    await expect(controller.paymentReversals("70", "25")).resolves.toEqual({
      items: [REVERSALS_GOLDEN.row],
      nextCursor: REVERSALS_GOLDEN.nextCursor,
      sourceHead: REVERSALS_GOLDEN.sourceHead,
    });
    expect(dbMocks.listPaymentReversalsAfter).toHaveBeenCalledWith({}, 70n, 25);
  });

});
