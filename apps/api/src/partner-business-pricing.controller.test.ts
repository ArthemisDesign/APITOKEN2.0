import { beforeEach, describe, expect, it, vi } from "vitest";

// Commerce re-checks what sales already checked. That redundancy is the point: this route is
// authenticated only as "sales", so without an independent ownership proof a defect on the sales
// side would be enough to reprice any customer in the system.
const dbMocks = vi.hoisted(() => ({
  getReferralAttributionCode: vi.fn(),
  getPricingView: vi.fn(),
  convertCustomerToBusiness: vi.fn(),
  setBusinessPricingBundle: vi.fn(),
  listCustomerProviderDiscounts: vi.fn(),
}));

vi.mock("@claude-api/db", () => ({
  ...dbMocks,
  isDiscountProviderId: (value: string) =>
    ["anthropic", "openai", "google", "kimi", "glm"].includes(value),
  BusinessCustomerNotFoundError: class BusinessCustomerNotFoundError extends Error {},
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
    userId: USER,
    referralCode: "partnercode",
    ceilingPercent: 70,
    discountPercent: 60,
    ...overrides,
  };
}

describe("partner-driven B2B pricing in commerce", () => {
  beforeEach(() => {
    for (const mock of Object.values(dbMocks)) mock.mockReset();
    dbMocks.listCustomerProviderDiscounts.mockResolvedValue([]);
    dbMocks.getPricingView.mockResolvedValue({ customerType: "b2b", discountPercent: 60 });
    dbMocks.setBusinessPricingBundle.mockResolvedValue({ engineAccountId: "acct_x", jobIds: ["job"] });
  });

  it("refuses a customer who is not the calling partner's referral", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("someone-elses-code");
    await expect(controller().partnerBusinessPricing(request())).rejects.toThrow(/not attributed/i);
    expect(dbMocks.setBusinessPricingBundle).not.toHaveBeenCalled();
    expect(dbMocks.convertCustomerToBusiness).not.toHaveBeenCalled();
  });

  it("refuses a customer with no referral attribution at all", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue(null);
    await expect(controller().partnerBusinessPricing(request())).rejects.toThrow(/not attributed/i);
    expect(dbMocks.setBusinessPricingBundle).not.toHaveBeenCalled();
  });

  it("re-enforces the ceiling instead of trusting the caller", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("partnercode");
    await expect(controller().partnerBusinessPricing(request({ discountPercent: 71 })))
      .rejects.toThrow(/exceeds the partner ceiling/i);
    expect(dbMocks.setBusinessPricingBundle).not.toHaveBeenCalled();
  });

  it("re-enforces the ceiling on every provider override, not just the default", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("partnercode");
    await expect(controller().partnerBusinessPricing(request({
      discountPercent: 50,
      providers: { anthropic: 50, kimi: 90 },
    }))).rejects.toThrow(/exceeds the partner ceiling/i);
    expect(dbMocks.setBusinessPricingBundle).not.toHaveBeenCalled();
  });

  it("rejects a provider id the engine would never match", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("partnercode");
    await expect(controller().partnerBusinessPricing(request({
      providers: { "anthropik": 10 },
    }))).rejects.toThrow();
    expect(dbMocks.setBusinessPricingBundle).not.toHaveBeenCalled();
  });

  it("converts a B2C referral once and does not re-apply the same default", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("partnercode");
    dbMocks.getPricingView
      .mockResolvedValueOnce({ customerType: "b2c", discountPercent: 50 })
      .mockResolvedValue({ customerType: "b2b", discountPercent: 60 });
    dbMocks.convertCustomerToBusiness.mockResolvedValue({ converted: true });

    const result = await controller().partnerBusinessPricing(request());
    expect(dbMocks.convertCustomerToBusiness).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ userId: USER, multiplierBp: 4000 }),
    );
    // Conversion already applied the default; re-sending it would be an empty bundle call.
    expect(dbMocks.setBusinessPricingBundle).not.toHaveBeenCalled();
    expect(result).toMatchObject({ converted: true, customerType: "b2b" });
  });

  it("refuses to convert without a base discount", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("partnercode");
    dbMocks.getPricingView.mockResolvedValue({ customerType: "b2c", discountPercent: 50 });
    await expect(controller().partnerBusinessPricing(request({
      discountPercent: undefined,
      providers: { kimi: 10 },
    }))).rejects.toThrow(/requires the default discount/i);
    expect(dbMocks.convertCustomerToBusiness).not.toHaveBeenCalled();
  });

  it("applies provider overrides on an already-business customer without re-converting", async () => {
    dbMocks.getReferralAttributionCode.mockResolvedValue("partnercode");
    await controller().partnerBusinessPricing(request({
      discountPercent: undefined,
      providers: { kimi: 20, google: null },
    }));
    expect(dbMocks.convertCustomerToBusiness).not.toHaveBeenCalled();
    expect(dbMocks.setBusinessPricingBundle).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        userId: USER,
        // null drops the override back to the customer's default.
        providers: { kimi: 8000, google: null },
      }),
    );
  });
});
