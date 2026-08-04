import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, oauthUrl } from "./api";

afterEach(() => vi.unstubAllGlobals());

describe("browser API client", () => {
  it("sends authenticated browser requests with credentials", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ user: { id: "u" } }), {
      status: 200, headers: { "content-type": "application/json" },
    }));
    vi.stubGlobal("fetch", fetchMock);
    await api.me();
    expect(fetchMock).toHaveBeenCalledWith("https://backend.apitoken.sale/v1/auth/me", expect.objectContaining({
      credentials: "include", cache: "no-store",
    }));
  });

  it("keeps checkout amounts as exact strings", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ id: "checkout" }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await api.createCheckout("9007199254740993");
    const request = fetchMock.mock.calls[0]![1] as RequestInit;
    expect(JSON.parse(String(request.body))).toEqual({ amountUsd: "9007199254740993", provider: "platega" });
  });

  it("serializes API key policies without numeric money conversion", async () => {
    const fetchMock = vi.fn().mockImplementation(async () =>
      new Response(JSON.stringify({ id: "key" }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await api.createApiKey({
      label: "Production", spendLimitUsd: "9007199254740993.25",
      expiresAt: "2099-01-01T00:00:00.000Z", totpCode: "123456",
    });
    const request = fetchMock.mock.calls[0]![1] as RequestInit;
    expect(JSON.parse(String(request.body))).toEqual({
      label: "Production", spendLimitUsd: "9007199254740993.25",
      expiresAt: "2099-01-01T00:00:00.000Z", totpCode: "123456",
    });

    fetchMock.mockClear();
    await api.updateApiKeyPolicy("9d8ac711-43c0-47f1-95af-a4a8ad6a89fe", {
      spendLimitUsd: null,
      expiresAt: "2099-02-01T00:00:00.000Z",
      totpCode: "654321",
    });
    const [url, policyRequest] = fetchMock.mock.calls[0]! as [string, RequestInit];
    expect(url).toBe("https://backend.apitoken.sale/v1/api-keys/9d8ac711-43c0-47f1-95af-a4a8ad6a89fe/policy");
    expect(policyRequest.method).toBe("PATCH");
    expect(JSON.parse(String(policyRequest.body))).toEqual({
      spendLimitUsd: null,
      expiresAt: "2099-02-01T00:00:00.000Z",
      totpCode: "654321",
    });
  });

  it("updates only the display name through the authenticated profile endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ user: { id: "u", displayName: "Alice" } }), {
      status: 200, headers: { "content-type": "application/json" },
    }));
    vi.stubGlobal("fetch", fetchMock);
    await api.updateProfile("Alice");
    expect(fetchMock).toHaveBeenCalledWith("https://backend.apitoken.sale/v1/auth/me", expect.objectContaining({
      method: "PATCH", credentials: "include", body: JSON.stringify({ displayName: "Alice" }),
    }));
  });

  it("surfaces backend errors with their status", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ message: "authentication required" }), { status: 401 })));
    const failure = await api.me().catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toEqual(expect.objectContaining({ status: 401, message: "authentication required" }));
  });

  it("sends persisted referrals through password registration", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ user: { id: "u" }, verificationRequired: false }), {
      status: 200, headers: { "content-type": "application/json" },
    }));
    vi.stubGlobal("fetch", fetchMock);

    await api.register({ email: "referral@example.com", password: "password", referralCode: "partner-code" });

    expect(fetchMock).toHaveBeenCalledWith("https://backend.apitoken.sale/v1/auth/register", expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ email: "referral@example.com", password: "password", referralCode: "partner-code" }),
    }));
  });

  it("preserves invitation and referral codes in OAuth starts", () => {
    expect(oauthUrl("github", "invite-token", "partner-code")).toBe(
      "https://backend.apitoken.sale/v1/auth/github?invite=invite-token&ref=partner-code",
    );
  });
});
