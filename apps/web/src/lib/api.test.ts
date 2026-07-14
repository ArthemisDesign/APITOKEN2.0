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
    expect(JSON.parse(String(request.body))).toEqual({ amountUsd: "9007199254740993", provider: "cryptomus" });
  });

  it("surfaces backend errors with their status", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ message: "authentication required" }), { status: 401 })));
    const failure = await api.me().catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toEqual(expect.objectContaining({ status: 401, message: "authentication required" }));
  });

  it("preserves B2B invitation tokens in OAuth starts", () => {
    expect(oauthUrl("github", "invite-token")).toBe("https://backend.apitoken.sale/v1/auth/github?invite=invite-token");
  });
});
