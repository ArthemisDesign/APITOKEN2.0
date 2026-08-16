import { beforeEach, describe, expect, it, vi } from "vitest";

const dbMocks = vi.hoisted(() => ({
  listCrmReferralRegistrations: vi.fn(),
}));

vi.mock("@claude-api/db", () => ({
  listCrmReferralRegistrations: dbMocks.listCrmReferralRegistrations,
}));

import { CrmBridgeService } from "./crm-bridge.service.js";

const EXTERNAL_REF = "10000000-0000-4000-8000-000000000001";
const ALIAS = "r_opaquealias000000000000";

function config() {
  const values: Record<string, unknown> = {
    SALES_API_URL: "https://sales.example.test",
    SALES_CONTROL_KEY: "s".repeat(32),
    CRM_REFERRAL_PARTNER_CODE: "crm-owner",
    PUBLIC_APP_BASE_URL: "https://apitoken.sale",
  };
  return { get: vi.fn((key: string) => values[key]) } as never;
}

function salesAliasResponse() {
  return new Response(JSON.stringify({
    source: "crm",
    externalRef: EXTERNAL_REF,
    code: ALIAS,
    partnerId: "20000000-0000-4000-8000-000000000002",
    createdAt: "2026-08-16T10:00:00.000Z",
  }), { status: 200, headers: { "content-type": "application/json" } });
}

describe("Commerce CRM referral bridge", () => {
  beforeEach(() => {
    dbMocks.listCrmReferralRegistrations.mockReset();
    vi.restoreAllMocks();
  });

  it("issues an opaque referral destination without leaking the CRM reference", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(salesAliasResponse());
    const service = new CrmBridgeService({} as never, {} as never, config());

    const link = await service.ensureReferralLink(EXTERNAL_REF);

    expect(link).toEqual({
      schemaVersion: 1,
      externalRef: EXTERNAL_REF,
      referralAlias: ALIAS,
      destinationUrl: `https://apitoken.sale/?ref=${ALIAS}&utm_source=crm&utm_medium=direct_sales&utm_campaign=crm-referral&utm_content=${ALIAS}`,
      createdAt: "2026-08-16T10:00:00.000Z",
    });
    const [, request] = fetchMock.mock.calls[0]!;
    expect(JSON.parse(String(request?.body))).toEqual({
      source: "crm",
      externalRef: EXTERNAL_REF,
      partnerCode: "crm-owner",
    });
    expect(link.destinationUrl).not.toContain(EXTERNAL_REF);
  });

  it("returns every attributed registration and batches live engine state", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(salesAliasResponse());
    dbMocks.listCrmReferralRegistrations.mockResolvedValue([
      {
        candidateId: "30000000-0000-4000-8000-000000000003",
        email: "first@example.test",
        emailVerified: true,
        registeredAt: new Date("2026-08-16T10:05:00.000Z"),
        customerStatus: "active",
        customerType: "b2b",
        defaultMultiplierBp: 4_500,
        providerOverrides: [{ providerId: "openai", multiplierBp: 3_500 }],
        paidTopupNano: 20_000_000_000n,
        refundedNano: 1_000_000_000n,
        usageSpentNano: 8_000_000_000n,
        customerFundedSpentNano: 7_500_000_000n,
        engineAccountId: "acct_first",
        projectedEngineStatus: "active",
      },
      {
        candidateId: "40000000-0000-4000-8000-000000000004",
        email: "forwarded@example.test",
        emailVerified: false,
        registeredAt: new Date("2026-08-16T10:06:00.000Z"),
        customerStatus: "disabled",
        customerType: "b2c",
        defaultMultiplierBp: 4_000,
        providerOverrides: [],
        paidTopupNano: 0n,
        refundedNano: 0n,
        usageSpentNano: 0n,
        customerFundedSpentNano: 0n,
        engineAccountId: "acct_second",
        projectedEngineStatus: "disabled",
      },
    ]);
    const engine = { getAccounts: vi.fn().mockResolvedValue([
      {
        account: "acct_first",
        balance_nano: "12500000000",
        spent_nano: "8000000000",
        reserved_nano: "0",
        balance: "12.5",
        mult_bp: 4_000,
        status: "active",
        handle: null,
      },
      {
        account: "acct_second",
        balance_nano: "0",
        spent_nano: "0",
        reserved_nano: "0",
        balance: "0",
        mult_bp: 4_000,
        status: "disabled",
        handle: null,
      },
    ]) };
    const service = new CrmBridgeService({} as never, engine as never, config());

    const profile = await service.referralProfile(EXTERNAL_REF);

    expect(profile.attributionStatus).toBe("ambiguous");
    expect(profile.registrations).toHaveLength(2);
    expect(engine.getAccounts).toHaveBeenCalledWith(["acct_first", "acct_second"]);
    expect(profile.registrations[0]).toMatchObject({
      email: "first@example.test",
      pricing: {
        defaultMultiplierBp: 4_000,
        defaultDiscountBps: 6_000,
        defaultState: "live",
        providerOverrides: [{ providerId: "openai", multiplierBp: 3_500, discountBps: 6_500 }],
      },
      money: {
        paidTopupNano: "20000000000",
        refundedNano: "1000000000",
        usageSpentNano: "8000000000",
        customerFundedSpentNano: "7500000000",
        balanceNano: "12500000000",
        liveState: "complete",
      },
    });
  });

  it("does not fabricate a zero balance when the engine is unavailable", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(salesAliasResponse());
    dbMocks.listCrmReferralRegistrations.mockResolvedValue([{
      candidateId: "30000000-0000-4000-8000-000000000003",
      email: "first@example.test",
      emailVerified: true,
      registeredAt: new Date(),
      customerStatus: "active",
      customerType: "b2c",
      defaultMultiplierBp: 4_000,
      providerOverrides: [],
      paidTopupNano: 0n,
      refundedNano: 0n,
      usageSpentNano: 0n,
      customerFundedSpentNano: 0n,
      engineAccountId: "acct_first",
      projectedEngineStatus: "active",
    }]);
    const engine = { getAccounts: vi.fn().mockRejectedValue(new Error("offline")) };
    const service = new CrmBridgeService({} as never, engine as never, config());

    const profile = await service.referralProfile(EXTERNAL_REF);

    expect(profile.registrations[0]?.money.balanceNano).toBeNull();
    expect(profile.registrations[0]?.money.liveState).toBe("unavailable");
    expect(profile.registrations[0]?.engineStatus).toBeNull();
    expect(profile.registrations[0]?.pricing.defaultState).toBe("saved");
  });
});
