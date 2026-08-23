import { describe, expect, it } from "vitest";
import { DEFAULT_PARTNER_TERMS, partnerOnboardingPayload } from "./partner-onboarding-form";

describe("partner onboarding terms", () => {
  it("starts every partner able to build a Team and set B2B terms, bounded by the two ceilings", () => {
    expect(partnerOnboardingPayload(DEFAULT_PARTNER_TERMS)).toEqual({
      commissionBps: 1000,
      authority: {
        teamOverrideMaxBps: 2000,
        teamInvitesEnabled: true,
        b2bEnabled: true,
        b2bMaxDiscountBps: 5000,
        b2bCanDelegate: true,
      },
    });
  });

  it("maps percentages to bounded basis points", () => {
    expect(partnerOnboardingPayload({ commission: "12.5", teamMaximum: "15", b2bMaximum: "65" })).toEqual({
      commissionBps: 1250,
      authority: {
        teamOverrideMaxBps: 1500,
        teamInvitesEnabled: true,
        b2bEnabled: true,
        b2bMaxDiscountBps: 6500,
        b2bCanDelegate: true,
      },
    });
  });

  it("switches B2B off for one partner through a zero ceiling", () => {
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, b2bMaximum: "0" })?.authority).toMatchObject({
      b2bEnabled: false,
      b2bMaxDiscountBps: 0,
      b2bCanDelegate: false,
      teamInvitesEnabled: true,
    });
  });

  it("rejects values outside platform hard limits", () => {
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, teamMaximum: "20.01" })).toBeNull();
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, commission: "100.01" })).toBeNull();
    expect(partnerOnboardingPayload({ ...DEFAULT_PARTNER_TERMS, b2bMaximum: "95.01" })).toBeNull();
  });
});
