import type { SalesDatabase } from "@claude-api/sales-db";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CommerceService, ReferralProfile } from "./commerce.service.js";

const getPartnerActivity = vi.hoisted(() => vi.fn());

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...original,
    getPartnerActivity: (...args: unknown[]) => getPartnerActivity(...args),
  };
});

const { AdminController } = await import("./admin.controller.js");

const PARTNER_ID = "11111111-1111-4111-8111-111111111111";
const USER_ID = "22222222-2222-4222-8222-222222222222";

function profile(): ReferralProfile {
  return {
    userId: USER_ID,
    email: "customer@example.test",
    customerType: "b2c",
    multiplierBp: 5_000,
    discountPercent: 50,
    referralFloorBps: 0,
    cumulativeTopupNano: "1000000000",
    balanceNano: "1000000000",
    status: "active",
  };
}

function controller(referralProfiles: CommerceService["referralProfiles"]): InstanceType<typeof AdminController> {
  return new AdminController(
    { pool: { query: vi.fn() } } as unknown as SalesDatabase,
    { get: vi.fn() } as never,
    { referralProfiles } as CommerceService,
    { invalidatePartnerSessions: vi.fn() } as never,
  );
}

beforeEach(() => {
  getPartnerActivity.mockReset();
  getPartnerActivity.mockResolvedValue([{
    type: "referral",
    at: "2026-08-22T00:00:00.000Z",
    amountNano: null,
    label: "New referral 22222222",
    meta: { commerceUserId: USER_ID },
  }]);
});

describe("managed-admin referral activity identity", () => {
  it("replaces the UUID-derived label with authoritative email and removes the raw id", async () => {
    const referralProfiles = vi.fn().mockResolvedValue(new Map([[USER_ID, profile()]]));
    const result = await controller(referralProfiles).partnerActivity(PARTNER_ID) as {
      events: Array<{ label: string; email: string | null; userMask: string | null; meta: Record<string, unknown> }>;
    };
    expect(referralProfiles).toHaveBeenCalledWith([USER_ID]);
    expect(result.events[0]).toMatchObject({
      label: "New referral customer@example.test",
      email: "customer@example.test",
      userMask: "user-22222222…",
    });
    expect(result.events[0]?.meta).not.toHaveProperty("commerceUserId");
  });

  it("falls back to the short mask when Commerce is unavailable", async () => {
    const result = await controller(vi.fn().mockResolvedValue(new Map())).partnerActivity(PARTNER_ID) as {
      events: Array<{ label: string; email: string | null }>;
    };
    expect(result.events[0]).toMatchObject({
      label: "New referral user-22222222…",
      email: null,
    });
  });
});
