import type { SalesDatabase } from "@claude-api/sales-db";
import { NotFoundException } from "@nestjs/common";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  resolveCommercePartnerMembership: vi.fn(),
  findPendingCommercePartnerInvitation: vi.fn(),
  declineCommercePartnerInvitation: vi.fn(),
}));

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...original,
    resolveCommercePartnerMembership: (...args: unknown[]) => mocks.resolveCommercePartnerMembership(...args),
    findPendingCommercePartnerInvitation: (...args: unknown[]) => mocks.findPendingCommercePartnerInvitation(...args),
    declineCommercePartnerInvitation: (...args: unknown[]) => mocks.declineCommercePartnerInvitation(...args),
  };
});

const { CommercePartnerController } = await import("./commerce-partner.controller.js");

const COMMERCE_USER_ID = "11111111-1111-4111-8111-111111111111";
const INVITE_ID = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function controller(): InstanceType<typeof CommercePartnerController> {
  const database = { pool: { query: vi.fn() } } as unknown as SalesDatabase;
  return new CommercePartnerController(database, { get: () => 1_000 } as never, {} as never);
}

beforeEach(() => {
  mocks.resolveCommercePartnerMembership.mockReset();
  mocks.findPendingCommercePartnerInvitation.mockReset();
  mocks.declineCommercePartnerInvitation.mockReset();
});

describe("Team invitations are accepted explicitly", () => {
  it("reads a partner snapshot without activating a pending invitation", async () => {
    mocks.resolveCommercePartnerMembership.mockResolvedValue({ state: "unavailable", activated: false, partner: null });

    await controller().partnerSnapshot(COMMERCE_USER_ID);

    expect(mocks.resolveCommercePartnerMembership).toHaveBeenCalledWith(
      expect.anything(),
      { commerceUserId: COMMERCE_USER_ID, activate: false },
    );
  });

  it("returns the pending invitation terms without consuming them", async () => {
    mocks.findPendingCommercePartnerInvitation.mockResolvedValue({ id: INVITE_ID, commissionBps: 1_000 });

    await expect(controller().pendingInvitation(COMMERCE_USER_ID)).resolves.toEqual({
      invitation: { id: INVITE_ID, commissionBps: 1_000 },
    });
    expect(mocks.resolveCommercePartnerMembership).not.toHaveBeenCalled();
  });

  it("activates the membership only when the invitee accepts", async () => {
    mocks.resolveCommercePartnerMembership.mockResolvedValue({
      state: "active",
      activated: true,
      partner: {
        id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", commerceUserId: COMMERCE_USER_ID, programEnabled: true,
        programStartedAt: new Date("2026-08-23T09:00:00.000Z"), email: null, displayName: null, telegramId: null,
        telegramUsername: null, telegramPhotoUrl: null, status: "active", emailVerified: false,
        referralCode: "p_accept", parentPartnerId: null, commissionBps: 1_000, subCommissionBps: 1_000,
        teamOverrideMaxBps: 1_000, parentOverrideBps: 1_500, payoutMethod: null, payoutDetails: null,
        promoEnabled: false, promoMaxValueNano: 0n, promoMaxCount: 0, referralDiscountBps: 0,
        referralDiscountEnabled: false, b2bEnabled: false, b2bMaxDiscountBps: 0, teamInvitesEnabled: true,
        b2bCanDelegate: false, b2bGrantSourcePartnerId: null, createdAt: new Date("2026-08-23T09:00:00.000Z"),
      },
    });

    const result = await controller().acceptInvitation(COMMERCE_USER_ID) as { activated: boolean };

    expect(mocks.resolveCommercePartnerMembership).toHaveBeenCalledWith(
      expect.anything(),
      { commerceUserId: COMMERCE_USER_ID, activate: true },
    );
    expect(result.activated).toBe(true);
  });

  it("reports a missing invitation instead of inventing a membership", async () => {
    mocks.resolveCommercePartnerMembership.mockResolvedValue({ state: "unavailable", activated: false, partner: null });

    await expect(controller().acceptInvitation(COMMERCE_USER_ID)).rejects.toBeInstanceOf(NotFoundException);
  });

  it("declines only the invitee's own pending invitation", async () => {
    mocks.declineCommercePartnerInvitation.mockResolvedValue({ declined: true });

    await expect(controller().declineInvitation(COMMERCE_USER_ID, { inviteId: INVITE_ID })).resolves.toEqual({ declined: true });
    expect(mocks.declineCommercePartnerInvitation).toHaveBeenCalledWith(
      expect.anything(),
      { commerceUserId: COMMERCE_USER_ID, inviteId: INVITE_ID },
    );
  });
});
