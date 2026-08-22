import { beforeEach, describe, expect, it, vi } from "vitest";

const dbMocks = vi.hoisted(() => ({
  findActiveReferralCommerceAccountByEmail: vi.fn(),
  findActiveReferralCommerceAccountById: vi.fn(),
  listReferralCommerceAccountsByIds: vi.fn(),
}));

vi.mock("@claude-api/db", () => dbMocks);

import { ReferralService } from "./referral.service.js";

const ISO = "2026-08-20T12:00:00.000Z";
const OWNER_USER_ID = "10000000-0000-4000-8000-000000000001";
const OWNER_PARTNER_ID = "10000000-0000-4000-8000-000000000002";
const PARENT_PARTNER_ID = "10000000-0000-4000-8000-000000000003";
const OWNER_GRANT_ID = "10000000-0000-4000-8000-000000000004";
const REFERRAL_USER_ID = "20000000-0000-4000-8000-000000000001";
const TEAM_USER_ID = "30000000-0000-4000-8000-000000000001";
const TEAM_PARTNER_ID = "30000000-0000-4000-8000-000000000002";
const TEAM_GRANT_ID = "30000000-0000-4000-8000-000000000003";
const INVITEE_USER_ID = "40000000-0000-4000-8000-000000000001";
const CUSTOMER_USER_ID = "50000000-0000-4000-8000-000000000001";
const REQUESTER_PARTNER_ID = "50000000-0000-4000-8000-000000000002";
const SUBJECT_PARTNER_ID = "50000000-0000-4000-8000-000000000003";
const PAYOUT_PARTNER_ID = "60000000-0000-4000-8000-000000000001";

function account(
  id: string,
  email: string,
  overrides: Partial<{
    customerType: "b2c" | "b2b" | null;
    discountBps: number | null;
    providerDiscounts: Array<{ providerId: string; discountBps: number }>;
  }> = {},
) {
  return {
    id,
    email,
    emailVerified: true,
    status: "active" as const,
    customerType: overrides.customerType ?? "b2c" as const,
    discountBps: overrides.discountBps ?? 5_000,
    providerDiscounts: overrides.providerDiscounts ?? [],
  };
}

function membership() {
  return {
    partnerId: OWNER_PARTNER_ID,
    commerceUserId: OWNER_USER_ID,
    status: "active",
    programEnabled: true,
    programStartedAt: ISO,
    referralCode: "opaque-owner-code",
    parentPartnerId: PARENT_PARTNER_ID,
    commissionBps: 1_000,
    teamShareBps: 2_000,
    teamOverrideMaxBps: 2_000,
    teamInvitesEnabled: true,
    b2bEnabled: true,
    b2bMaxDiscountBps: 6_000,
    b2bCanDelegate: true,
    b2bGrantSourcePartnerId: OWNER_GRANT_ID,
    payoutMethod: "bsc_usdt",
    payoutDetails: { address: "0x0000000000000000000000000000000000000000" },
    createdAt: ISO,
  };
}

function request() {
  return {
    id: "70000000-0000-4000-8000-000000000001",
    requestType: "b2b_conversion",
    status: "pending",
    requesterPartnerId: REQUESTER_PARTNER_ID,
    requesterEmail: "stale-requester@sales.test",
    requesterDisplayName: "Stale Sales Name",
    subjectPartnerId: SUBJECT_PARTNER_ID,
    customerCommerceUserId: CUSTOMER_USER_ID,
    customerEmail: "stale-customer@sales.test",
    reason: "customer needs negotiated terms",
    stateSnapshot: { customerType: "b2c", discountPercent: 50 },
    requestedCommissionBps: null,
    requestedDiscountBps: 6_000,
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
    createdAt: ISO,
    updatedAt: ISO,
  };
}

function activeSnapshot() {
  return {
    state: "active",
    activated: false,
    membership: membership(),
    totals: {
      earnedNano: "100", directNano: "80", overrideNano: "20", adjustmentNano: "0",
      directAdjustmentNano: "0", overrideAdjustmentNano: "0", netNano: "100", directNetNano: "80",
      overrideNetNano: "20", paidNano: "0", pendingPayoutNano: "0", debtNano: "0", availableNano: "100",
      last30dSpendNano: "1000", last30dEarnedNano: "100", last30dAdjustmentNano: "0", last30dNetNano: "100",
    },
    referrals: [{
      commerceUserId: REFERRAL_USER_ID,
      userRef: "abcd1234",
      attributedAt: ISO,
      spendNano: "1000",
      earnedNano: "100",
      adjustmentNano: "0",
      netNano: "100",
      topupNano: "2000",
    }],
    team: [{
      id: TEAM_PARTNER_ID,
      commerceUserId: TEAM_USER_ID,
      programEnabled: true,
      programStartedAt: ISO,
      status: "active",
      commissionBps: 1_000,
      overrideBps: 2_000,
      teamOverrideMaxBps: 1_000,
      teamInvitesEnabled: false,
      b2bEnabled: true,
      b2bMaxDiscountBps: 5_000,
      b2bCanDelegate: false,
      b2bGrantSourcePartnerId: TEAM_GRANT_ID,
      referredUsers: 2,
      theirEarnedNano: "80",
      theirAdjustmentNano: "0",
      theirNetNano: "80",
      myOverrideNano: "20",
      myOverrideAdjustmentNano: "0",
      myOverrideNetNano: "20",
    }],
    earnings: {
      days: 30,
      daily: [{ date: "2026-08-20", spendNano: "1000", earnedNano: "100", adjustmentNano: "0", netNano: "100" }],
      providers: [{ providerId: "anthropic", events: 1, spendNano: "1000", earnedNano: "100" }],
      providerDaily: [{
        date: "2026-08-20",
        providers: [{ providerId: "anthropic", events: 1, spendNano: "1000", earnedNano: "100" }],
      }],
    },
    invitations: [{
      id: "80000000-0000-4000-8000-000000000001",
      commerceUserId: INVITEE_USER_ID,
      overrideBps: 1_000,
      teamOverrideMaxBps: 1_000,
      teamInvitesEnabled: false,
      b2bEnabled: false,
      b2bMaxDiscountBps: 0,
      b2bCanDelegate: false,
      expiresAt: ISO,
      consumedAt: null,
      revokedAt: null,
      createdAt: ISO,
    }],
    requests: [request()],
    payouts: [{
      id: "90000000-0000-4000-8000-000000000001",
      partnerId: PAYOUT_PARTNER_ID,
      amountNano: "100",
      status: "requested",
      method: "bsc_usdt",
      details: { address: "0x0000000000000000000000000000000000000000" },
      requestedAt: ISO,
      decidedAt: null,
      paidAt: null,
      adminNote: null,
      txHash: null,
      chainStatus: null,
    }],
    period: {
      now: ISO,
      current: { key: "2026-08-2", start: ISO, end: ISO, accruedNano: "100", adjustmentNano: "0", netNano: "100" },
      locked: [],
      nextPayout: { date: ISO, estimatedNano: "100" },
      lifetimeEarnedNano: "100",
      lifetimeAdjustmentNano: "0",
      lifetimeNetNano: "100",
      lifetimePaidNano: "0",
      debtNano: "0",
      payableNano: "100",
      unpaidNano: "100",
    },
    periodHistory: [{
      key: "2026-08-2",
      index: 2,
      start: ISO,
      end: ISO,
      phase: "accruing",
      payoutDate: ISO,
      earnedNano: "100",
      adjustmentNano: "0",
      netNano: "100",
    }],
    payoutPolicy: { minPayoutNano: "50000000000", lockDays: 7, windowDays: 3 },
  };
}

function nestedKeys(value: unknown): string[] {
  if (Array.isArray(value)) return value.flatMap(nestedKeys);
  if (!value || typeof value !== "object") return [];
  return Object.entries(value).flatMap(([key, nested]) => [key, ...nestedKeys(nested)]);
}

describe("Referral Commerce identity projection", () => {
  beforeEach(() => {
    dbMocks.findActiveReferralCommerceAccountByEmail.mockReset();
    dbMocks.findActiveReferralCommerceAccountById.mockReset();
    dbMocks.listReferralCommerceAccountsByIds.mockReset();
  });

  it("enriches every customer-facing row by current Commerce email and removes internal identity fields", async () => {
    const sales = { call: vi.fn().mockResolvedValue(activeSnapshot()) };
    dbMocks.listReferralCommerceAccountsByIds.mockResolvedValue([
      account(REFERRAL_USER_ID, "referral@example.test", {
        customerType: "b2b",
        discountBps: 6_000,
        providerDiscounts: [{ providerId: "openai", discountBps: 7_000 }],
      }),
      account(TEAM_USER_ID, "team@example.test"),
      account(INVITEE_USER_ID, "invitee@example.test"),
      account(CUSTOMER_USER_ID, "customer@example.test"),
    ]);
    const service = new ReferralService({} as never, sales as never);

    const result = await service.partnerSnapshot(OWNER_USER_ID, "current-owner@example.test");

    expect(result).toMatchObject({
      state: "active",
      membership: { email: "current-owner@example.test" },
      referrals: [{
        email: "referral@example.test",
        customerType: "b2b",
        discountBps: 6_000,
        providerDiscounts: [{ providerId: "openai", discountBps: 7_000 }],
      }],
      team: [{ email: "team@example.test" }],
      invitations: [{ email: "invitee@example.test" }],
      requests: [{
        requesterEmail: "current-owner@example.test",
        customerEmail: "customer@example.test",
      }],
      payouts: [{ id: "90000000-0000-4000-8000-000000000001" }],
    });
    expect(dbMocks.listReferralCommerceAccountsByIds).toHaveBeenCalledWith({}, [
      REFERRAL_USER_ID,
      TEAM_USER_ID,
      INVITEE_USER_ID,
      CUSTOMER_USER_ID,
    ]);
    expect(nestedKeys(result)).not.toEqual(expect.arrayContaining([
      "partnerId",
      "commerceUserId",
      "parentPartnerId",
      "b2bGrantSourcePartnerId",
      "requesterPartnerId",
      "subjectPartnerId",
      "userRef",
    ]));
    const serialized = JSON.stringify(result);
    for (const internalId of [
      OWNER_USER_ID,
      OWNER_PARTNER_ID,
      PARENT_PARTNER_ID,
      OWNER_GRANT_ID,
      REFERRAL_USER_ID,
      TEAM_USER_ID,
      TEAM_PARTNER_ID,
      TEAM_GRANT_ID,
      INVITEE_USER_ID,
      CUSTOMER_USER_ID,
      REQUESTER_PARTNER_ID,
      SUBJECT_PARTNER_ID,
      PAYOUT_PARTNER_ID,
    ]) {
      expect(serialized).not.toContain(internalId);
    }
  });

  it("uses the current authenticated email even for a disabled partner record", async () => {
    const sales = {
      call: vi.fn().mockResolvedValue({ state: "disabled", membership: { ...membership(), status: "suspended" } }),
    };
    const service = new ReferralService({} as never, sales as never);

    const result = await service.partnerSnapshot(OWNER_USER_ID, "renamed-owner@example.test");

    expect(result).toMatchObject({
      state: "disabled",
      membership: { email: "renamed-owner@example.test", status: "suspended" },
    });
    expect(JSON.stringify(result)).not.toContain(OWNER_USER_ID);
    expect(JSON.stringify(result)).not.toContain(OWNER_PARTNER_ID);
    expect(dbMocks.listReferralCommerceAccountsByIds).not.toHaveBeenCalled();
  });

  it("resolves a Team target by email and strips both Commerce and Sales partner IDs from the mutation result", async () => {
    const teamAccount = account(TEAM_USER_ID, "team@example.test");
    dbMocks.findActiveReferralCommerceAccountByEmail.mockResolvedValue(teamAccount);
    const sales = { call: vi.fn().mockResolvedValue({
      invitation: {
        id: "80000000-0000-4000-8000-000000000001",
        inviterPartnerId: OWNER_PARTNER_ID,
        commerceUserId: TEAM_USER_ID,
        overrideBps: 2_000,
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
        expiresAt: ISO,
        createdAt: ISO,
        created: true,
      },
    }) };
    const service = new ReferralService({} as never, sales as never);
    const authority = {
      teamOverrideMaxBps: 1_000,
      teamInvitesEnabled: false,
      b2bEnabled: false,
      b2bMaxDiscountBps: 0,
      b2bCanDelegate: false,
    };

    const result = await service.inviteTeamMember(OWNER_USER_ID, {
      email: "TEAM@example.test",
      overrideBps: 2_000,
      authority,
    });

    expect(dbMocks.findActiveReferralCommerceAccountByEmail).toHaveBeenCalledWith({}, "TEAM@example.test");
    expect(sales.call).toHaveBeenCalledWith(
      `partner/${OWNER_USER_ID}/team-invitations`,
      expect.anything(),
      {
        method: "POST",
        body: { inviteeCommerceUserId: TEAM_USER_ID, overrideBps: 2_000, authority },
      },
    );
    expect(result).toMatchObject({ invitation: { email: "team@example.test", created: true } });
    expect(JSON.stringify(result)).not.toContain(OWNER_PARTNER_ID);
    expect(JSON.stringify(result)).not.toContain(TEAM_USER_ID);
  });

  it("projects the Admin partner directory by current Commerce email only", async () => {
    const sales = { call: vi.fn().mockResolvedValue({ items: [{
      partnerId: OWNER_PARTNER_ID,
      commerceUserId: OWNER_USER_ID,
      programEnabled: true,
      programStartedAt: ISO,
      status: "active",
      referralCode: "opaque-owner-code",
      commissionBps: 1_000,
      teamOverrideMaxBps: 2_000,
      teamShareBps: null,
      parentPartnerId: null,
      referredUsers: 1,
      teamSize: 0,
      earnedNano: "100",
      adjustmentNano: "0",
      netNano: "100",
      debtNano: "0",
      payableNano: "100",
      paidNano: "0",
      b2bEnabled: true,
      b2bMaxDiscountBps: 6_000,
      teamInvitesEnabled: true,
      b2bCanDelegate: false,
      createdAt: ISO,
    }] }) };
    dbMocks.listReferralCommerceAccountsByIds.mockResolvedValue([
      account(OWNER_USER_ID, "current-owner@example.test"),
    ]);
    const service = new ReferralService({} as never, sales as never);

    const result = await service.adminPartners();

    expect(result).toMatchObject({ items: [{ email: "current-owner@example.test" }] });
    expect(nestedKeys(result)).not.toEqual(expect.arrayContaining([
      "displayName",
      "partnerId",
      "commerceUserId",
      "parentPartnerId",
    ]));
    expect(JSON.stringify(result)).not.toContain(OWNER_USER_ID);
    expect(JSON.stringify(result)).not.toContain(OWNER_PARTNER_ID);
  });

  it("uses current Commerce emails in the admin request queue and removes producer identity fields", async () => {
    const adminRequest = { ...request(), requesterPartnerId: OWNER_PARTNER_ID };
    const sales = { call: vi.fn((path: string) => {
      if (path.startsWith("admin/requests")) {
        return Promise.resolve({ items: [adminRequest], nextCursor: null });
      }
      if (path === "admin/partners") {
        return Promise.resolve({ items: [{
          partnerId: OWNER_PARTNER_ID,
          commerceUserId: OWNER_USER_ID,
          programEnabled: true,
          programStartedAt: ISO,
          status: "active",
          referralCode: "opaque-owner-code",
          commissionBps: 1_000,
          teamOverrideMaxBps: 2_000,
          teamShareBps: null,
          parentPartnerId: null,
          referredUsers: 1,
          teamSize: 0,
          earnedNano: "100",
          adjustmentNano: "0",
          netNano: "100",
          debtNano: "0",
          payableNano: "100",
          paidNano: "0",
          b2bEnabled: true,
          b2bMaxDiscountBps: 6_000,
          teamInvitesEnabled: true,
          b2bCanDelegate: false,
          createdAt: ISO,
        }] });
      }
      throw new Error(`unexpected path: ${path}`);
    }) };
    dbMocks.listReferralCommerceAccountsByIds.mockResolvedValue([
      account(OWNER_USER_ID, "current-requester@example.test"),
      account(CUSTOMER_USER_ID, "current-customer@example.test"),
    ]);
    const service = new ReferralService({} as never, sales as never);

    const result = await service.adminRequests("?status=pending&limit=25");

    expect(result).toMatchObject({
      items: [{
        requesterEmail: "current-requester@example.test",
        customerEmail: "current-customer@example.test",
      }],
      nextCursor: null,
    });
    expect(JSON.stringify(result)).not.toContain("stale-requester@sales.test");
    expect(JSON.stringify(result)).not.toContain("stale-customer@sales.test");
    expect(nestedKeys(result)).not.toEqual(expect.arrayContaining([
      "requesterPartnerId",
      "subjectPartnerId",
      "customerCommerceUserId",
      "requesterDisplayName",
    ]));
  });
});
