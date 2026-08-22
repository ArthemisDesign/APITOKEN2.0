import { describe, expect, it } from "vitest";
import { DEFAULT_PARTNER_TERMS, partnerOnboardingPayload } from "./partner-onboarding-form";

describe("partner onboarding terms", () => {
  it("maps percentages to bounded basis points and preserves delegated authority", () => {
    expect(partnerOnboardingPayload({
      commission: "10",
      teamMaximum: "20",
      teamInvitesEnabled: true,
      b2bEnabled: true,
      b2bMaximum: "65",
      b2bCanDelegate: true,
    })).toEqual({
      commissionBps: 1000,
      authority: {
        teamOverrideMaxBps: 2000,
        teamInvitesEnabled: true,
        b2bEnabled: true,
        b2bMaxDiscountBps: 6500,
        b2bCanDelegate: true,
      },
    });
  });

  it("removes the B2B ceiling and delegation when self-service is disabled", () => {
    expect(partnerOnboardingPayload({
      ...DEFAULT_PARTNER_TERMS,
      b2bEnabled: false,
      b2bMaximum: "95",
      b2bCanDelegate: true,
    })?.authority).toMatchObject({
      b2bEnabled: false,
      b2bMaxDiscountBps: 0,
      b2bCanDelegate: false,
    });
  });

  it("rejects values outside platform hard limits", () => {
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, teamMaximum: "20.01" })).toBeNull();
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, commission: "100.01" })).toBeNull();
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, b2bEnabled: true, b2bMaximum: "95.01" })).toBeNull();
  });
});
