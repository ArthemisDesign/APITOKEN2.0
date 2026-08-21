import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Partner } from "@claude-api/sales-db";

const resolvePartnerSessionMock = vi.fn();
const revokePartnerSessionMock = vi.fn();

vi.mock("@claude-api/sales-db", async (importOriginal) => {
  const original = await importOriginal<typeof import("@claude-api/sales-db")>();
  return {
    ...original,
    resolvePartnerSession: (...args: unknown[]) => resolvePartnerSessionMock(...args),
    revokePartnerSession: (...args: unknown[]) => revokePartnerSessionMock(...args),
  };
});

const { AuthService, partnerView } = await import("./auth.service.js");

const TOKEN = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_TOKEN = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

function partnerFixture(id: string): Partner {
  return {
    id,
    email: "seller@example.test",
    displayName: "Seller",
    telegramId: "42424242",
    telegramUsername: "seller",
    telegramPhotoUrl: null,
    status: "active",
    emailVerified: true,
    referralCode: "sellercode",
    parentPartnerId: null,
    commissionBps: 1000,
    subCommissionBps: 1000,
    payoutMethod: null,
    payoutDetails: null,
    promoEnabled: false,
    promoMaxValueNano: 0n,
    promoMaxCount: 0,
    referralDiscountBps: 0,
    referralDiscountEnabled: false,
    b2bEnabled: false,
    b2bMaxDiscountBps: 0,
    createdAt: new Date(1_784_500_000_000),
  };
}

function authService(cacheTtlSeconds: number): InstanceType<typeof AuthService> {
  const config = { get: (key: string) => (key === "SALES_SESSION_CACHE_TTL_SECONDS" ? cacheTtlSeconds : undefined) };
  return new AuthService(null as never, config as never);
}

describe("AuthService session cache", () => {
  beforeEach(() => {
    resolvePartnerSessionMock.mockReset();
    revokePartnerSessionMock.mockReset();
    vi.useRealTimers();
  });

  it("resolves from PostgreSQL once and serves repeats from the cache within the TTL", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_784_500_000_000);
    resolvePartnerSessionMock.mockResolvedValue({ sessionId: "session-1", partner: partnerFixture("partner-1") });
    const service = authService(30);
    const first = await service.authenticate(TOKEN);
    const second = await service.authenticate(TOKEN);
    expect(first?.sessionId).toBe("session-1");
    expect(second?.sessionId).toBe("session-1");
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(1);
  });

  it("re-resolves after the TTL expires", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_784_500_000_000);
    resolvePartnerSessionMock.mockResolvedValue({ sessionId: "session-1", partner: partnerFixture("partner-1") });
    const service = authService(30);
    await service.authenticate(TOKEN);
    vi.setSystemTime(1_784_500_031_000);
    await service.authenticate(TOKEN);
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(2);
  });

  it("does not cache when the TTL is zero", async () => {
    resolvePartnerSessionMock.mockResolvedValue({ sessionId: "session-1", partner: partnerFixture("partner-1") });
    const service = authService(0);
    await service.authenticate(TOKEN);
    await service.authenticate(TOKEN);
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(2);
  });

  it("never caches a failed resolution", async () => {
    resolvePartnerSessionMock.mockResolvedValue(null);
    const service = authService(30);
    expect(await service.authenticate(TOKEN)).toBeNull();
    expect(await service.authenticate(TOKEN)).toBeNull();
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(2);
  });

  it("invalidates the cached session on logout", async () => {
    resolvePartnerSessionMock.mockResolvedValue({ sessionId: "session-1", partner: partnerFixture("partner-1") });
    revokePartnerSessionMock.mockResolvedValue(undefined);
    const service = authService(30);
    const resolved = await service.authenticate(TOKEN);
    await service.logout(resolved!.sessionId, "partner-1");
    await service.authenticate(TOKEN);
    expect(revokePartnerSessionMock).toHaveBeenCalledTimes(1);
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(2);
  });

  it("keeps tokens isolated from each other", async () => {
    resolvePartnerSessionMock
      .mockResolvedValueOnce({ sessionId: "session-1", partner: partnerFixture("partner-1") })
      .mockResolvedValueOnce({ sessionId: "session-2", partner: partnerFixture("partner-2") });
    const service = authService(30);
    const first = await service.authenticate(TOKEN);
    const second = await service.authenticate(OTHER_TOKEN);
    expect(first?.sessionId).toBe("session-1");
    expect(second?.sessionId).toBe("session-2");
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(2);
  });

  it("drops every cached session of a partner on explicit invalidation", async () => {
    resolvePartnerSessionMock
      .mockResolvedValueOnce({ sessionId: "session-1", partner: partnerFixture("partner-1") })
      .mockResolvedValueOnce({ sessionId: "session-2", partner: partnerFixture("partner-1") })
      .mockResolvedValue({ sessionId: "session-3", partner: partnerFixture("partner-1") });
    const service = authService(30);
    await service.authenticate(TOKEN);
    await service.authenticate(OTHER_TOKEN);
    service.invalidatePartnerSessions("partner-1");
    await service.authenticate(TOKEN);
    await service.authenticate(OTHER_TOKEN);
    expect(resolvePartnerSessionMock).toHaveBeenCalledTimes(4);
  });
});

describe("partner session view", () => {
  it("keeps the B2B grant in the authenticated partner payload", () => {
    const view = partnerView({
      ...partnerFixture("partner-b2b"),
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_000,
    });
    expect(view.b2bEnabled).toBe(true);
    expect(view.b2bMaxDiscountBps).toBe(4_000);
  });
});
