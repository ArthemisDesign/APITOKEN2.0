import { describe, expect, it, vi } from "vitest";
import {
  isShortReferralCode,
  resolveShortReferral,
  safeReferralDestination,
} from "./crm-referral-gateway";

describe("CRM short referral gateway", () => {
  it("accepts only a seven-character lowercase code led by a digit", () => {
    expect(isShortReferralCode("3kgj45g")).toBe(true);
    expect(isShortReferralCode("pricing")).toBe(false);
    expect(isShortReferralCode("3KGJ45G")).toBe(false);
    expect(isShortReferralCode("3kgj45")).toBe(false);
    expect(isShortReferralCode("3kgj45gg")).toBe(false);
    expect(isShortReferralCode("akgj45g")).toBe(false);
  });

  it("resolves through the CRM tracker without forwarding browser state", async () => {
    const destination = "https://apitoken.sale/?ref=r_opaque&utm_content=r_opaque";
    const calls: Array<[Parameters<typeof fetch>[0], Parameters<typeof fetch>[1]]> = [];
    const fetcher: typeof fetch = async (input, init) => {
      calls.push([input, init]);
      return new Response(null, { status: 303, headers: { location: destination } });
    };

    await expect(resolveShortReferral("3kgj45g", fetcher)).resolves.toBe(destination);
    expect(calls).toHaveLength(1);
    const [url, init] = calls[0]!;
    expect(url).toBe("https://crm.apitoken.sale/r/3kgj45g");
    expect(init?.redirect).toBe("manual");
    expect(init?.cache).toBe("no-store");
    expect(init?.headers).toEqual({ accept: "text/html" });
  });

  it("never calls CRM for an ordinary root page", async () => {
    const fetcher = vi.fn();
    await expect(resolveShortReferral("pricing", fetcher as typeof fetch)).resolves.toBeNull();
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("rejects off-origin, malformed and un-attributed redirects", () => {
    expect(safeReferralDestination("https://evil.example/?ref=x&utm_content=x")).toBeNull();
    expect(safeReferralDestination("https://apitoken.sale/login?ref=x&utm_content=x")).toBeNull();
    expect(safeReferralDestination("https://apitoken.sale/?ref=x&utm_content=y")).toBeNull();
    expect(safeReferralDestination("https://apitoken.sale/")).toBeNull();
    expect(safeReferralDestination("not a URL")).toBeNull();
  });

  it("fails closed on upstream errors and non-redirect responses", async () => {
    const failing = vi.fn(async () => { throw new Error("offline"); });
    const ok = vi.fn(async () => new Response("unexpected", { status: 200 }));
    await expect(resolveShortReferral("3kgj45g", failing as typeof fetch)).resolves.toBeNull();
    await expect(resolveShortReferral("3kgj45g", ok as typeof fetch)).resolves.toBeNull();
  });
});
