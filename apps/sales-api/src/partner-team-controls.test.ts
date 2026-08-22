import type { SalesDatabase } from "@claude-api/sales-db";
import { BadRequestException, ForbiddenException, NotFoundException, UnprocessableEntityException } from "@nestjs/common";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createPartnerInvite: vi.fn(),
  updateDirectTeamMemberAuthority: vi.fn(),
}));

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...original,
    createPartnerInvite: (...args: unknown[]) => mocks.createPartnerInvite(...args),
    updateDirectTeamMemberAuthority: (...args: unknown[]) => mocks.updateDirectTeamMemberAuthority(...args),
  };
});

const {
  PartnerTeamAuthorityError,
  TeamMemberNotFoundError,
} = await import("@claude-api/sales-db");
const { PartnerController } = await import("./partner.controller.js");

function controller(): InstanceType<typeof PartnerController> {
  const database = { pool: { query: vi.fn() } } as unknown as SalesDatabase;
  const config = {
    get: (key: string) => key === "DEFAULT_COMMISSION_BPS"
      ? 1_000
      : "https://partners.apitoken.sale",
  };
  return new PartnerController(
    database,
    config as never,
    {} as never,
    { invalidatePartnerSessions: vi.fn() } as never,
  );
}

function auth(maximumBps = 2_000) {
  return {
    partner: {
      id: "11111111-1111-4111-8111-111111111111",
      commissionBps: 1_500,
      teamOverrideMaxBps: maximumBps,
      subCommissionBps: 1_000,
      referralDiscountEnabled: false,
      referralDiscountBps: 0,
      teamInvitesEnabled: true,
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_000,
      b2bCanDelegate: true,
    },
  } as never;
}

beforeEach(() => {
  mocks.createPartnerInvite.mockReset();
  mocks.updateDirectTeamMemberAuthority.mockReset();
  mocks.createPartnerInvite.mockImplementation(async (_database: unknown, input: Record<string, unknown>) => ({
    id: "invite-id",
    partnerId: input.partnerId,
    code: input.code,
    telegramUsername: input.telegramUsername,
    commissionBps: input.commissionBps,
    subCommissionBps: input.subCommissionBps,
    teamOverrideMaxBps: input.teamOverrideMaxBps,
    parentOverrideBps: input.parentOverrideBps,
    promoEnabled: false,
    promoMaxValueNano: 0n,
    promoMaxCount: 0,
    referralDiscountBps: 0,
    referralDiscountEnabled: false,
    b2bEnabled: false,
    b2bMaxDiscountBps: 0,
    teamInvitesEnabled: input.teamInvitesEnabled,
    b2bCanDelegate: false,
    expiresAt: input.expiresAt,
    consumedAt: null,
    consumedByPartnerId: null,
    createdAt: new Date(),
  }));
});

describe("partner Team controls", () => {
  it("keeps the deployed invite writer semantics during producer-first rollout", async () => {
    const result = await controller().createInvite(auth(1_500), {
      telegramUsername: "legacy_member",
      commissionBps: 900,
    }) as { commissionBps: number };
    expect(result.commissionBps).toBe(900);
    expect(mocks.createPartnerInvite).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      commissionBps: 900,
      parentOverrideBps: null,
      teamOverrideMaxBps: null,
    }));
  });

  it("ignores a forged member rate and snapshots the platform default 10%", async () => {
    const result = await controller().createTeamInvite(auth(1_500), {
      telegramUsername: "member_one",
      commissionBps: 9_000,
      overrideBps: 1_500,
      teamOverrideMaxBps: 1_200,
    }) as { commissionBps: number; overrideBps: number; teamOverrideMaxBps: number };

    expect(result).toMatchObject({ commissionBps: 1_000, overrideBps: 1_500, teamOverrideMaxBps: 1_200 });
    expect(mocks.createPartnerInvite).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      commissionBps: 1_000,
      parentOverrideBps: 1_500,
      teamOverrideMaxBps: 1_200,
    }));
  });

  it("accepts the global 20% boundary", async () => {
    await expect(controller().createTeamInvite(auth(), {
      telegramUsername: "member_two",
      overrideBps: 2_000,
      teamOverrideMaxBps: 2_000,
    })).resolves.toMatchObject({ overrideBps: 2_000, teamOverrideMaxBps: 2_000 });
  });

  it("rejects 20.01% and a grant above the inviter's narrower ceiling", async () => {
    await expect(controller().createTeamInvite(auth(), {
      telegramUsername: "member_three",
      overrideBps: 2_001,
    })).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller().createTeamInvite(auth(1_100), {
      telegramUsername: "member_four",
      overrideBps: 1_101,
    })).rejects.toBeInstanceOf(UnprocessableEntityException);
    expect(mocks.createPartnerInvite).not.toHaveBeenCalled();
  });

  it("maps a non-direct member update to not found", async () => {
    mocks.updateDirectTeamMemberAuthority.mockRejectedValue(new TeamMemberNotFoundError());
    await expect(controller().updateTeamMember(
      auth(),
      "22222222-2222-4222-8222-222222222222",
      { overrideBps: 500 },
    )).rejects.toBeInstanceOf(NotFoundException);
  });

  it("maps forbidden Team delegation to a forbidden response", async () => {
    mocks.updateDirectTeamMemberAuthority.mockRejectedValue(
      new PartnerTeamAuthorityError("your Team invitation authority cannot be delegated"),
    );
    await expect(controller().updateTeamMember(
      auth(),
      "22222222-2222-4222-8222-222222222222",
      { teamInvitesEnabled: true },
    )).rejects.toBeInstanceOf(ForbiddenException);
  });

  it("blocks new invitations when the platform disabled that authority", async () => {
    const current = auth() as any;
    current.partner.teamInvitesEnabled = false;
    await expect(controller().createTeamInvite(current, {
      telegramUsername: "member_five",
    })).rejects.toBeInstanceOf(ForbiddenException);
    expect(mocks.createPartnerInvite).not.toHaveBeenCalled();
  });

  it("passes only bounded delegated B2B authority into the invite", async () => {
    await expect(controller().createTeamInvite(auth(), {
      telegramUsername: "member_six",
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_001,
    })).rejects.toBeInstanceOf(UnprocessableEntityException);
    await controller().createTeamInvite(auth(), {
      telegramUsername: "member_seven",
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_000,
      b2bCanDelegate: true,
    });
    expect(mocks.createPartnerInvite).toHaveBeenLastCalledWith(expect.anything(), expect.objectContaining({
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_000,
      b2bCanDelegate: true,
    }));
  });
});
