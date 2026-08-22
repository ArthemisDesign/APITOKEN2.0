import { describe, expect, it } from "vitest";
import { adminApplicationDecisionSchema } from "./schemas.js";

describe("admin application authority schema", () => {
  it("accepts all bounded onboarding authority in one approval", () => {
    expect(adminApplicationDecisionSchema.safeParse({
      action: "approve",
      commissionBps: 1_500,
      subCommissionBps: 900,
      teamOverrideMaxBps: 1_200,
      teamInvitesEnabled: true,
      b2bEnabled: true,
      b2bMaxDiscountBps: 7_000,
      b2bCanDelegate: true,
      note: "verified pipeline",
    }).success).toBe(true);
  });

  it("rejects authority retained without a B2B grant", () => {
    expect(adminApplicationDecisionSchema.safeParse({
      action: "approve",
      b2bMaxDiscountBps: 1_000,
    }).success).toBe(false);
    expect(adminApplicationDecisionSchema.safeParse({
      action: "approve",
      b2bCanDelegate: true,
    }).success).toBe(false);
  });
});
