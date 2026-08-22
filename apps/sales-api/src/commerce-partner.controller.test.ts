import type { Partner, SalesDatabase } from "@claude-api/sales-db";
import { ConflictException } from "@nestjs/common";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  resolveCommercePartnerMembership: vi.fn(),
  createCommerceTeamInvite: vi.fn(),
  revokeCommerceTeamInvite: vi.fn(),
  findPartnerByCommerceUserId: vi.fn(),
  updateCommercePartnerWallet: vi.fn(),
  insertSalesAudit: vi.fn(),
  listPartnersWithAggregates: vi.fn(),
}));

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...original,
    resolveCommercePartnerMembership: (...args: unknown[]) => mocks.resolveCommercePartnerMembership(...args),
    createCommerceTeamInvite: (...args: unknown[]) => mocks.createCommerceTeamInvite(...args),
    revokeCommerceTeamInvite: (...args: unknown[]) => mocks.revokeCommerceTeamInvite(...args),
    findPartnerByCommerceUserId: (...args: unknown[]) => mocks.findPartnerByCommerceUserId(...args),
    updateCommercePartnerWallet: (...args: unknown[]) => mocks.updateCommercePartnerWallet(...args),
    insertSalesAudit: (...args: unknown[]) => mocks.insertSalesAudit(...args),
    listPartnersWithAggregates: (...args: unknown[]) => mocks.listPartnersWithAggregates(...args),
  };
});

const { CommercePartnerConflictError } = await import("@claude-api/sales-db");
const { CommercePartnerController } = await import("./commerce-partner.controller.js");

const COMMERCE_USER_ID = "11111111-1111-4111-8111-111111111111";
const INVITEE_USER_ID = "22222222-2222-4222-8222-222222222222";
const INVITE_ID = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function partner(): Partner {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    commerceUserId: COMMERCE_USER_ID,
    programEnabled: true,
    programStartedAt: new Date("2026-08-22T12:00:00.000Z"),
    email: null,
    displayName: null,
    telegramId: null,
    telegramUsername: null,
    telegramPhotoUrl: null,
    status: "active",
    emailVerified: false,
    referralCode: "p_contract",
    parentPartnerId: null,
    commissionBps: 1_000,
    subCommissionBps: 1_000,
    teamOverrideMaxBps: 2_000,
    parentOverrideBps: null,
    payoutMethod: null,
    payoutDetails: null,
    promoEnabled: false,
    promoMaxValueNano: 0n,
    promoMaxCount: 0,
    referralDiscountBps: 0,
    referralDiscountEnabled: false,
    b2bEnabled: false,
    b2bMaxDiscountBps: 0,
    teamInvitesEnabled: true,
    b2bCanDelegate: false,
    b2bGrantSourcePartnerId: null,
    createdAt: new Date("2026-08-22T12:00:00.000Z"),
  };
}

function controller(): InstanceType<typeof CommercePartnerController> {
  const database = { pool: { query: vi.fn() } } as unknown as SalesDatabase;
  const config = {
    get: (key: string) => key === "DEFAULT_COMMISSION_BPS" ? 1_000 : 1_000,
  };
  return new CommercePartnerController(database, config as never, {} as never);
}

beforeEach(() => {
  mocks.resolveCommercePartnerMembership.mockReset();
  mocks.createCommerceTeamInvite.mockReset();
  mocks.revokeCommerceTeamInvite.mockReset();
  mocks.findPartnerByCommerceUserId.mockReset();
  mocks.updateCommercePartnerWallet.mockReset();
  mocks.insertSalesAudit.mockReset();
  mocks.listPartnersWithAggregates.mockReset();
});

describe("Commerce partner internal contract", () => {
  it("returns a concise unavailable state for an ordinary Dashboard account", async () => {
    mocks.resolveCommercePartnerMembership.mockResolvedValue({
      state: "unavailable",
      activated: false,
      partner: null,
    });
    await expect(controller().resolve({ commerceUserId: COMMERCE_USER_ID })).resolves.toEqual({
      state: "unavailable",
      activated: false,
      membership: null,
    });
  });

  it("exposes Commerce identity and authority without promo or Telegram fields", async () => {
    mocks.resolveCommercePartnerMembership.mockResolvedValue({
      state: "active",
      activated: false,
      partner: partner(),
    });
    const response = await controller().resolve({ commerceUserId: COMMERCE_USER_ID }) as {
      membership: Record<string, unknown>;
    };
    expect(response.membership).toMatchObject({
      commerceUserId: COMMERCE_USER_ID,
      commissionBps: 1_000,
      teamOverrideMaxBps: 2_000,
    });
    expect(response.membership).not.toHaveProperty("telegramUsername");
    expect(response.membership).not.toHaveProperty("promoEnabled");
    expect(response.membership).not.toHaveProperty("referralDiscountBps");
  });

  it("passes only the platform rate and bounded delegated controls into a Team invitation", async () => {
    mocks.createCommerceTeamInvite.mockResolvedValue({
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      inviterPartnerId: partner().id,
      commerceUserId: INVITEE_USER_ID,
      overrideBps: 2_000,
      teamOverrideMaxBps: 1_500,
      teamInvitesEnabled: true,
      b2bEnabled: false,
      b2bMaxDiscountBps: 0,
      b2bCanDelegate: false,
      expiresAt: new Date("2026-09-22T12:00:00.000Z"),
      createdAt: new Date("2026-08-22T12:00:00.000Z"),
      created: true,
    });
    await controller().inviteTeamMember(COMMERCE_USER_ID, {
      inviteeCommerceUserId: INVITEE_USER_ID,
      overrideBps: 2_000,
      authority: {
        teamOverrideMaxBps: 1_500,
        teamInvitesEnabled: true,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
    });
    expect(mocks.createCommerceTeamInvite).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      inviterCommerceUserId: COMMERCE_USER_ID,
      inviteeCommerceUserId: INVITEE_USER_ID,
      defaultCommissionBps: 1_000,
      overrideBps: 2_000,
    }));
  });

  it("maps competing account-bound invitations to HTTP conflict", async () => {
    mocks.createCommerceTeamInvite.mockRejectedValue(
      new CommercePartnerConflictError("this account already has another open Team invitation"),
    );
    await expect(controller().inviteTeamMember(COMMERCE_USER_ID, {
      inviteeCommerceUserId: INVITEE_USER_ID,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
    })).rejects.toBeInstanceOf(ConflictException);
  });

  it("revokes an invitation only through the session-derived inviter membership", async () => {
    mocks.revokeCommerceTeamInvite.mockResolvedValue({
      id: INVITE_ID,
      commerceUserId: INVITEE_USER_ID,
      revokedAt: new Date("2026-08-22T12:00:00.000Z"),
      revoked: true,
    });
    await controller().revokeTeamInvitation(COMMERCE_USER_ID, INVITE_ID);
    expect(mocks.revokeCommerceTeamInvite).toHaveBeenCalledWith(expect.anything(), {
      inviterCommerceUserId: COMMERCE_USER_ID,
      inviteId: INVITE_ID,
    });
  });

  it("writes a BSC wallet only for an active Commerce membership and audits the change", async () => {
    const current = partner();
    const updated = {
      ...current,
      payoutMethod: "usdt-bep20",
      payoutDetails: {
        network: "BSC",
        asset: "USDT (BEP-20)",
        address: "0x1111111111111111111111111111111111111111",
      },
    };
    mocks.findPartnerByCommerceUserId.mockResolvedValue(current);
    mocks.updateCommercePartnerWallet.mockResolvedValue(updated);

    await controller().updateWallet(COMMERCE_USER_ID, {
      address: "0x1111111111111111111111111111111111111111",
    });
    expect(mocks.updateCommercePartnerWallet).toHaveBeenCalledWith(expect.anything(), {
      commerceUserId: COMMERCE_USER_ID,
      address: "0x1111111111111111111111111111111111111111",
    });
  });
});
