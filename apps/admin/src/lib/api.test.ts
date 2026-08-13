import { afterEach, describe, expect, it, vi } from "vitest";
import { api, mutationResources } from "./api";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("api", () => {
  it("always bypasses the browser cache for operational reads", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ ok: true })));
    vi.stubGlobal("fetch", fetchMock);

    await api<{ ok: boolean }>("/capacity");

    expect(fetchMock).toHaveBeenCalledWith("/capacity", expect.objectContaining({
      method: "GET",
      cache: "no-store",
    }));
  });
});

describe("mutationResources", () => {
  it("maps row actions back to the list resource invalidated by the producer", () => {
    expect(mutationResources("/admin/users/u-1/status")).toEqual(["/admin/users"]);
    expect(mutationResources("/admin/business-invites/invite-1/revoke")).toEqual(["/admin/business-invites"]);
    expect(mutationResources("/proxy-admin/renew")).toEqual(["/proxy-admin/inventory"]);
    expect(mutationResources("/gemini-subs/profile-1/disabled")).toEqual(["/gemini-subs"]);
  });

  it("refreshes both OpenKeys catalog projections after a warehouse action", () => {
    expect(mutationResources("/openkeys-admin/keys")).toEqual([
      "/openkeys-admin/keys",
      "/openkeys-admin/sellers",
      "/openkeys-admin/paying-keys",
      "/openkeys-admin/lookup",
    ]);
  });

  it("keeps already-list-shaped mutation paths unchanged", () => {
    expect(mutationResources("/admin/users?limit=50")).toEqual(["/admin/users"]);
  });
});
