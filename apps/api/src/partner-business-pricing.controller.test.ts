import { beforeEach, describe, expect, it, vi } from "vitest";

const errors = vi.hoisted(() => ({
  Authorization: class PartnerBusinessPricingAuthorizationError extends Error {},
  Conflict: class PartnerBusinessPricingConflictError extends Error {},
  Request: class PartnerBusinessPricingRequestError extends Error {},
  MissingBusinessCustomer: class BusinessCustomerNotFoundError extends Error {},
}));

const dbMocks = vi.hoisted(() => ({
  applySalesPartnerBusinessPricing: vi.fn(),
}));

vi.mock("@claude-api/db", () => ({
  ...dbMocks,
  isDiscountProviderId: (value: string) =>
    ["anthropic", "openai", "google", "kimi", "glm"].includes(value),
  BusinessCustomerNotFoundError: errors.MissingBusinessCustomer,
  PartnerBusinessPricingAuthorizationError: errors.Authorization,
  PartnerBusinessPricingConflictError: errors.Conflict,
  PartnerBusinessPricingRequestError: errors.Request,
  listUsageEventsAfter: vi.fn(),
  listPaidTopupsV2After: vi.fn(),
  listPaymentReversalsAfter: vi.fn(),
  listPaidTopupsAfter: vi.fn(),
  listReferralAttributionsAfter: vi.fn(),
  listReferralProfiles: vi.fn(),
  setReferralFloor: vi.fn(),
}));

import { SalesFeedController } from "./sales-feed.controller.js";

const USER = "00000000-0000-4000-8000-000000000099";

function controller() {
  return new SalesFeedController({} as never, {} as never);
}

function request(overrides: Record<string, unknown> = {}) {
  return {
    operationRef: "partner-effect:00000000-0000-4000-8000-000000000001",
    userId: USER,
    referralCode: "partnercode",
    ceilingPercent: 70,
    discountPercent: 60,
    actorId: "admin:operator@example.com",
    reason: "approved partner request 0001",
    ...overrides,
  };
}

describe("partner-driven B2B pricing in commerce", () => {
  beforeEach(() => {
    dbMocks.applySalesPartnerBusinessPricing.mockReset();
    dbMocks.applySalesPartnerBusinessPricing.mockResolvedValue({
      operationRef: "partner-effect:00000000-0000-4000-8000-000000000001",
      idempotentReplay: false,
      userId: USER,
      converted: true,
      customerType: "b2b",
      discountPercent: 60,
      providers: {},
    });
  });

  it("passes the stable operation and real actor to the atomic Commerce writer", async () => {
    const result = await controller().partnerBusinessPricing(request({
      providers: { kimi: 20, google: null },
    }));
    expect(dbMocks.applySalesPartnerBusinessPricing).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        operationRef: "partner-effect:00000000-0000-4000-8000-000000000001",
        userId: USER,
        actorId: "admin:operator@example.com",
        providers: { kimi: 20, google: null },
      }),
    );
    expect(result).toMatchObject({ converted: true, customerType: "b2b", idempotentReplay: false });
  });

  it("keeps operationRef optional for the producer-first rollout", async () => {
    await controller().partnerBusinessPricing(request({ operationRef: undefined }));
    expect(dbMocks.applySalesPartnerBusinessPricing).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ operationRef: undefined }),
    );
  });

  it("rejects an unknown provider before touching the database", async () => {
    await expect(controller().partnerBusinessPricing(request({
      providers: { anthropik: 10 },
    }))).rejects.toThrow(/invalid partner business pricing payload/i);
    expect(dbMocks.applySalesPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("rejects an empty mutation before touching the database", async () => {
    await expect(controller().partnerBusinessPricing(request({
      discountPercent: undefined,
      providers: undefined,
    }))).rejects.toThrow(/invalid partner business pricing payload/i);
    expect(dbMocks.applySalesPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("maps ownership and ceiling failures to forbidden", async () => {
    dbMocks.applySalesPartnerBusinessPricing.mockRejectedValue(
      new errors.Authorization("requested discount exceeds the partner ceiling"),
    );
    await expect(controller().partnerBusinessPricing(request())).rejects.toMatchObject({ status: 403 });
  });

  it("maps operation-ref payload drift to conflict", async () => {
    dbMocks.applySalesPartnerBusinessPricing.mockRejectedValue(
      new errors.Conflict("operation ref was already used for another pricing request"),
    );
    await expect(controller().partnerBusinessPricing(request())).rejects.toMatchObject({ status: 409 });
  });

  it("maps malformed operations and unavailable business accounts to bad request", async () => {
    dbMocks.applySalesPartnerBusinessPricing.mockRejectedValueOnce(
      new errors.Request("converting a referral to B2B requires the default discount"),
    );
    await expect(controller().partnerBusinessPricing(request())).rejects.toMatchObject({ status: 400 });

    dbMocks.applySalesPartnerBusinessPricing.mockRejectedValueOnce(
      new errors.MissingBusinessCustomer("referral has no provisioned business account yet"),
    );
    await expect(controller().partnerBusinessPricing(request())).rejects.toMatchObject({ status: 400 });
  });
});
