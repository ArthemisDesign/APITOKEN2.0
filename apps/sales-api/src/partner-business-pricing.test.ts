import type { SalesDatabase } from "@claude-api/sales-db";
import { ForbiddenException, UnprocessableEntityException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import { PartnerController } from "./partner.controller.js";
import type { CommerceService } from "./commerce.service.js";

// The ceiling is the whole safety property of the B2B grant: a deeper discount is margin the
// company gives away. It is never read from the request — only from the partner row an admin
// wrote — and commerce re-checks it independently.
function controller(commerce: Partial<CommerceService> = {}) {
  const database = { pool: { query: vi.fn() } } as unknown as SalesDatabase;
  return new PartnerController(
    database,
    { get: vi.fn() } as never,
    commerce as CommerceService,
    { invalidatePartnerSessions: vi.fn() } as never,
  );
}

function auth(overrides: Record<string, unknown> = {}) {
  return {
    partner: {
      id: "11111111-1111-4111-8111-111111111111",
      referralCode: "partnercode",
      b2bEnabled: true,
      b2bMaxDiscountBps: 7000,
      ...overrides,
    },
  } as never;
}

describe("partner B2B pricing authority", () => {
  it("refuses a partner without the grant before touching commerce", async () => {
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    await expect(api.setReferralBusinessPricing(
      auth({ b2bEnabled: false, b2bMaxDiscountBps: 0 }),
      "abcdef12",
      { discountPercent: 10 },
    )).rejects.toBeInstanceOf(ForbiddenException);
    expect(setPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("refuses a base discount deeper than the granted ceiling", async () => {
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    await expect(api.setReferralBusinessPricing(auth(), "abcdef12", { discountPercent: 71 }))
      .rejects.toBeInstanceOf(UnprocessableEntityException);
    expect(setPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("refuses a per-provider discount deeper than the ceiling", async () => {
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    // The base rate is legal here; a single over-ceiling provider must still stop the whole call,
    // otherwise the ceiling would only bind the value that happens to be checked first.
    await expect(api.setReferralBusinessPricing(
      auth(),
      "abcdef12",
      { discountPercent: 50, providers: { anthropic: 50, kimi: 95 } },
    )).rejects.toBeInstanceOf(UnprocessableEntityException);
    expect(setPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("takes the ceiling from the partner row, never from the request", async () => {
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    await expect(api.setReferralBusinessPricing(
      auth(),
      "abcdef12",
      // A forged ceiling in the body must not widen anything.
      { discountPercent: 90, ceilingPercent: 95, b2bMaxDiscountBps: 9500 } as never,
    )).rejects.toBeInstanceOf(UnprocessableEntityException);
    expect(setPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("rejects an empty change instead of calling commerce with nothing to do", async () => {
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    await expect(api.setReferralBusinessPricing(auth(), "abcdef12", {})).rejects.toThrow();
    expect(setPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("rejects a malformed referral reference", async () => {
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    await expect(api.setReferralBusinessPricing(auth(), "not-a-ref", { discountPercent: 10 }))
      .rejects.toThrow();
    expect(setPartnerBusinessPricing).not.toHaveBeenCalled();
  });

  it("allows a discount exactly at the ceiling", async () => {
    // Off-by-one at the boundary would quietly cost the partner their granted range.
    const setPartnerBusinessPricing = vi.fn();
    const api = controller({ setPartnerBusinessPricing });
    // Resolution of the referral happens after the ceiling check; reaching it proves 70 passed.
    await expect(api.setReferralBusinessPricing(auth(), "abcdef12", { discountPercent: 70 }))
      .rejects.not.toBeInstanceOf(UnprocessableEntityException);
  });
});
