import { afterEach, describe, expect, it, vi } from "vitest";
import { CommercePartnerPricingError, CommerceService } from "./commerce.service.js";

const USER_ID = "11111111-1111-4111-8111-111111111111";

function service(): CommerceService {
  const config = {
    get: (key: string) => key === "COMMERCE_BASE_URL"
      ? "https://backend.example.test"
      : "sales-control-key",
  };
  return new CommerceService(config as never);
}

function profile(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    userId: USER_ID,
    email: "customer@example.test",
    customerType: "b2c",
    multiplierBp: 5_000,
    discountPercent: 50,
    referralFloorBps: 0,
    cumulativeTopupNano: "1000000000",
    balanceNano: "500000000",
    status: "active",
    ...overrides,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("CommerceService.referralProfiles", () => {
  it("accepts and returns the authoritative Commerce email", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({
      items: [profile()],
    }), { status: 200, headers: { "content-type": "application/json" } })));

    const profiles = await service().referralProfiles([USER_ID]);
    expect(profiles.get(USER_ID)?.email).toBe("customer@example.test");
  });

  it("drops a malformed profile instead of presenting an unverified identity", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({
      items: [profile({ email: "not-an-email" })],
    }), { status: 200, headers: { "content-type": "application/json" } })));

    await expect(service().referralProfiles([USER_ID])).resolves.toEqual(new Map());
  });

  it("keeps the storefront available when Commerce is unavailable", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("network unavailable")));
    await expect(service().referralProfiles([USER_ID])).resolves.toEqual(new Map());
  });
});

describe("CommerceService.setPartnerBusinessPricing", () => {
  it("propagates durable operation evidence and validates the acknowledgement", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      operationRef: "partner-effect:request-1",
      idempotentReplay: true,
      userId: USER_ID,
      converted: true,
      customerType: "b2b",
      discountPercent: 40,
      providers: { anthropic: 35 },
    }), { status: 200, headers: { "content-type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(service().setPartnerBusinessPricing({
      operationRef: "partner-effect:request-1",
      userId: USER_ID,
      referralCode: "partner-code",
      ceilingPercent: 40,
      discountPercent: 40,
      providers: { anthropic: 35 },
      actorId: "admin@example.test",
      reason: "approved request",
    })).resolves.toMatchObject({
      operationRef: "partner-effect:request-1",
      idempotentReplay: true,
    });
    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(JSON.parse(String(request.body))).toMatchObject({
      operationRef: "partner-effect:request-1",
      actorId: "admin@example.test",
      reason: "approved request",
    });
  });

  it("preserves 409 as a terminal operation conflict", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("conflict", { status: 409 })));
    await expect(service().setPartnerBusinessPricing({
      operationRef: "partner-effect:request-1",
      userId: USER_ID,
      referralCode: "partner-code",
      ceilingPercent: 40,
      discountPercent: 40,
    })).rejects.toEqual(expect.objectContaining<Partial<CommercePartnerPricingError>>({ status: 409 }));
  });
});
