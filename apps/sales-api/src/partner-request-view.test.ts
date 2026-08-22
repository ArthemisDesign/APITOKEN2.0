import type { PartnerRequestView } from "@claude-api/sales-db";
import { describe, expect, it } from "vitest";
import { partnerRequestView } from "./partner-request-view.js";

const CUSTOMER_ID = "22222222-2222-4222-8222-222222222222";

function request(): PartnerRequestView {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    requestType: "b2b_conversion",
    status: "pending",
    requesterPartnerId: "11111111-1111-4111-8111-111111111111",
    requesterEmail: null,
    requesterDisplayName: null,
    subjectPartnerId: null,
    commerceUserId: CUSTOMER_ID,
    reason: "Negotiated customer terms",
    stateSnapshot: { customerType: "b2c", discountPercent: 50 },
    requestedCommissionBps: null,
    requestedDiscountBps: 4_000,
    approvedCommissionBps: null,
    approvedDiscountBps: null,
    reviewerActor: null,
    reviewerNote: null,
    reviewedAt: null,
    appliedAt: null,
    applyAttempts: 0,
    lastApplyError: null,
    version: 1,
    providerTerms: [],
    effect: null,
    createdAt: new Date("2026-08-22T09:00:00.000Z"),
    updatedAt: new Date("2026-08-22T09:00:00.000Z"),
  };
}

describe("partner request views", () => {
  it("keeps Commerce identity on the internal consumer contract", () => {
    expect(partnerRequestView(request(), null, { includeCommerceIdentity: true }))
      .toMatchObject({ customerCommerceUserId: CUSTOMER_ID, customerEmail: null });
  });

  it("does not expose Commerce UUIDs on legacy browser contracts", () => {
    expect(partnerRequestView(request(), "customer@example.test"))
      .not.toHaveProperty("customerCommerceUserId");
  });
});
