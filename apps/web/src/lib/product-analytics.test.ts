import { beforeEach, describe, expect, it, vi } from "vitest";

const { track } = vi.hoisted(() => ({ track: vi.fn() }));
vi.mock("@vercel/analytics", () => ({ track }));

import { checkoutAmountBucket, coarseAcquisition, trackFirstProductEvent } from "./product-analytics";

describe("product analytics privacy and first milestones", () => {
  beforeEach(() => {
    track.mockClear();
    const values = new Map<string, string>();
    vi.stubGlobal("window", {
      location: { search: "", pathname: "/", hostname: "apitoken.sale" },
      localStorage: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => { values.set(key, value); },
      },
    });
    vi.stubGlobal("document", { referrer: "", documentElement: { lang: "en" } });
  });

  it("tracks a browser milestone only once", () => {
    expect(trackFirstProductEvent("login", "First Login", { method: "password" })).toBe(true);
    expect(trackFirstProductEvent("login", "First Login", { method: "password" })).toBe(false);
    expect(track).toHaveBeenCalledTimes(1);
  });

  it("keeps only bounded campaign data and a clean landing path", () => {
    window.location.search = "?utm_source=Newsletter_1&utm_medium=some%40email&ref=SECRET";
    window.location.pathname = "/ru/plans";
    expect(coarseAcquisition()).toEqual({
      source: "newsletter_1", medium: "other", landing_path: "/ru/plans", language: "ru", has_referral: true,
    });
  });

  it("uses coarse checkout amount buckets", () => {
    expect(["49", "50", "100", "250", "500", "1000"].map(checkoutAmountBucket))
      .toEqual(["under_50", "50_99", "100_249", "250_499", "500_999", "1000_plus"]);
  });
});
