import { BadRequestException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import type { AuthUserView } from "@claude-api/contracts";
import type { RequestAuth } from "./auth.guard.js";
import { ReferralController } from "./referral.controller.js";

const SESSION_USER_ID = "10000000-0000-4000-8000-000000000001";

function currentAuth(): RequestAuth {
  const user: AuthUserView = {
    id: SESSION_USER_ID,
    email: "owner@example.test",
    displayName: "Owner",
    emailVerified: true,
    passwordEnabled: true,
    engineAccountStatus: "active",
    customerType: "b2b",
    totpEnabled: true,
  };
  return { sessionId: "session-1", user };
}

function controller() {
  const service = {
    partnerSnapshot: vi.fn(),
    inviteTeamMember: vi.fn(),
    revokeTeamInvitation: vi.fn(),
    updateTeamMember: vi.fn(),
    requestCommission: vi.fn(),
    requestB2B: vi.fn(),
    setBusinessPricing: vi.fn(),
    updateWallet: vi.fn(),
  };
  return { controller: new ReferralController(service as never), service };
}

describe("Referral session identity boundary", () => {
  it("derives the partner identity and current email only from the authenticated session", async () => {
    const fake = controller();
    fake.service.partnerSnapshot.mockResolvedValue({ state: "unavailable", membership: null });

    await fake.controller.snapshot(currentAuth());

    expect(fake.service.partnerSnapshot).toHaveBeenCalledWith(SESSION_USER_ID, "owner@example.test");
  });

  it("passes the authenticated owner to a valid Team invitation", async () => {
    const fake = controller();
    fake.service.inviteTeamMember.mockResolvedValue({ invitation: { id: "opaque" } });
    const body = {
      email: "member@example.test",
      overrideBps: 2_000,
      authority: {
        teamOverrideMaxBps: 2_000,
        teamInvitesEnabled: true,
        b2bEnabled: true,
        b2bMaxDiscountBps: 5_000,
        b2bCanDelegate: false,
      },
    };

    await fake.controller.inviteTeamMember(currentAuth(), body);

    expect(fake.service.inviteTeamMember).toHaveBeenCalledWith(SESSION_USER_ID, body);
  });

  it("rejects an injected owner UUID instead of forwarding it to the service", () => {
    const fake = controller();
    const maliciousBody = {
      email: "member@example.test",
      commerceUserId: "20000000-0000-4000-8000-000000000002",
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
    };

    expect(() => fake.controller.inviteTeamMember(currentAuth(), maliciousBody))
      .toThrow(BadRequestException);
    expect(fake.service.inviteTeamMember).not.toHaveBeenCalled();
  });
});
