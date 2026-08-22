import type { PartnerRequestView, SalesDatabase } from "@claude-api/sales-db";
import { BadRequestException } from "@nestjs/common";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CommerceService, ReferralProfile } from "./commerce.service.js";

const mocks = vi.hoisted(() => ({
  createCommissionChangeRequest: vi.fn(),
  createB2BPartnerRequest: vi.fn(),
  decidePartnerRequest: vi.fn(),
  resolveReferredUserByPrefix: vi.fn(),
}));

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...original,
    createCommissionChangeRequest: (...args: unknown[]) => mocks.createCommissionChangeRequest(...args),
    createB2BPartnerRequest: (...args: unknown[]) => mocks.createB2BPartnerRequest(...args),
    decidePartnerRequest: (...args: unknown[]) => mocks.decidePartnerRequest(...args),
    resolveReferredUserByPrefix: (...args: unknown[]) => mocks.resolveReferredUserByPrefix(...args),
  };
});

const { AdminController } = await import("./admin.controller.js");
const { PartnerController } = await import("./partner.controller.js");

const PARTNER_ID = "11111111-1111-4111-8111-111111111111";
const USER_ID = "22222222-2222-4222-8222-222222222222";
const REQUEST_ID = "33333333-3333-4333-8333-333333333333";

function request(overrides: Partial<PartnerRequestView> = {}): PartnerRequestView {
  return {
    id: REQUEST_ID,
    requestType: "commission_change",
    status: "pending",
    requesterPartnerId: PARTNER_ID,
    requesterEmail: "partner@example.test",
    requesterDisplayName: "Partner",
    subjectPartnerId: PARTNER_ID,
    commerceUserId: null,
    reason: "Qualified growth",
    stateSnapshot: { currentCommissionBps: 1_000 },
    requestedCommissionBps: 1_500,
    requestedDiscountBps: null,
    approvedCommissionBps: null,
    approvedDiscountBps: null,
    reviewerActor: null,
    reviewerNote: null,
    reviewedAt: null,
    appliedAt: null,
    applyAttempts: 0,
    lastApplyError: null,
    version: 1,
    createdAt: new Date("2026-08-22T09:00:00.000Z"),
    updatedAt: new Date("2026-08-22T09:00:00.000Z"),
    providerTerms: [],
    effect: null,
    ...overrides,
  };
}

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

function commerce(): CommerceService {
  return {
    referralProfiles: vi.fn().mockResolvedValue(new Map([[USER_ID, profile()]])),
  } as unknown as CommerceService;
}

function partnerController(commerceService = commerce()): InstanceType<typeof PartnerController> {
  return new PartnerController(
    { pool: { query: vi.fn() } } as unknown as SalesDatabase,
    { get: vi.fn() } as never,
    commerceService,
    { invalidatePartnerSessions: vi.fn() } as never,
  );
}

function adminController(commerceService = commerce()): InstanceType<typeof AdminController> {
  return new AdminController(
    { pool: { query: vi.fn() } } as unknown as SalesDatabase,
    { get: vi.fn() } as never,
    commerceService,
    { invalidatePartnerSessions: vi.fn() } as never,
  );
}

beforeEach(() => {
  for (const mock of Object.values(mocks)) mock.mockReset();
});

describe("partner request HTTP boundary", () => {
  it("requires an idempotency key and forwards the authenticated partner identity", async () => {
    mocks.createCommissionChangeRequest.mockResolvedValue(request());
    await expect(partnerController().requestCommissionChange(
      { partner: { id: PARTNER_ID } } as never,
      undefined,
      { requestedCommissionBps: 1_500, reason: "Qualified growth" },
    )).rejects.toBeInstanceOf(BadRequestException);

    await partnerController().requestCommissionChange(
      { partner: { id: PARTNER_ID } } as never,
      "request-key-123",
      { requestedCommissionBps: 1_500, reason: "Qualified growth" },
    );
    expect(mocks.createCommissionChangeRequest).toHaveBeenCalledWith(expect.anything(), {
      requesterPartnerId: PARTNER_ID,
      requestedCommissionBps: 1_500,
      reason: "Qualified growth",
      idempotencyKey: "request-key-123",
    });
  });

  it("proves referral ownership, snapshots Commerce state and returns email rather than user UUID", async () => {
    mocks.resolveReferredUserByPrefix.mockResolvedValue(USER_ID);
    mocks.createB2BPartnerRequest.mockResolvedValue(request({
      requestType: "b2b_conversion",
      subjectPartnerId: null,
      commerceUserId: USER_ID,
      requestedCommissionBps: null,
      requestedDiscountBps: 4_000,
      providerTerms: [{
        providerId: "anthropic",
        requestedDiscountBps: 3_500,
        approvedDiscountBps: undefined,
      }],
    }));
    const result = await partnerController().requestReferralB2B(
      { partner: { id: PARTNER_ID } } as never,
      USER_ID.slice(0, 8),
      "request-key-456",
      {
        discountPercent: 40,
        providers: { anthropic: 35 },
        reason: "Negotiated customer terms",
      },
    ) as { request: Record<string, unknown> };

    expect(mocks.resolveReferredUserByPrefix).toHaveBeenCalledWith(expect.anything(), PARTNER_ID, "22222222");
    expect(mocks.createB2BPartnerRequest).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      requesterPartnerId: PARTNER_ID,
      commerceUserId: USER_ID,
      requestType: "b2b_conversion",
      requestedDiscountBps: 4_000,
      providers: { anthropic: 3_500 },
      stateSnapshot: { customerType: "b2c", discountPercent: 50 },
    }));
    expect(result.request.customerEmail).toBe("customer@example.test");
    expect(result.request).not.toHaveProperty("commerceUserId");
  });

  it("propagates the authenticated admin actor and mandatory decision note", async () => {
    mocks.decidePartnerRequest.mockResolvedValue(request({
      status: "applied",
      approvedCommissionBps: 1_400,
      reviewerActor: "admin:operator@example.test",
      reviewerNote: "Volume verified",
      reviewedAt: new Date("2026-08-22T10:00:00.000Z"),
      appliedAt: new Date("2026-08-22T10:00:00.000Z"),
    }));
    await adminController().decidePartnerRequestEndpoint(
      REQUEST_ID,
      "admin:operator@example.test",
      { action: "approve", note: "Volume verified", commissionBps: 1_400 },
    );
    expect(mocks.decidePartnerRequest).toHaveBeenCalledWith(expect.anything(), {
      requestId: REQUEST_ID,
      action: "approve",
      reviewerActor: "admin:operator@example.test",
      reviewerNote: "Volume verified",
      approvedCommissionBps: 1_400,
    });
  });
});
