// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { captureReferralCode, referralBootstrapScript, storedReferralCode } from "./referral";

const STORAGE_KEY = "apitoken_ref";
const NOW = new Date("2026-08-04T12:00:00.000Z").getTime();
const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

describe("referral persistence", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.history.replaceState(null, "", "/");
    vi.spyOn(Date, "now").mockReturnValue(NOW);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("captures the initial referral before hydration", () => {
    window.history.replaceState(null, "", "/?ref=partner_123#pricing");

    window.eval(referralBootstrapScript);

    expect(storedReferralCode()).toBe("partner_123");
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY)!)).toEqual({ code: "partner_123", at: NOW });
  });

  it("uses the latest distinct click without extending the same code", () => {
    captureReferralCode("first-code");
    vi.mocked(Date.now).mockReturnValue(NOW + 1_000);
    captureReferralCode("first-code");
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY)!)).toEqual({ code: "first-code", at: NOW });

    captureReferralCode("new-code");
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY)!)).toEqual({ code: "new-code", at: NOW + 1_000 });
  });

  it("removes expired attribution and ignores malformed or invalid values", () => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ code: "expired-code", at: NOW - THIRTY_DAYS_MS - 1 }));
    expect(storedReferralCode()).toBeUndefined();
    expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull();

    window.localStorage.setItem(STORAGE_KEY, "not-json");
    expect(storedReferralCode()).toBeUndefined();
    captureReferralCode("no spaces allowed");
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("not-json");
  });

  it("fails safely when browser storage is unavailable", () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        getItem: () => { throw new Error("storage disabled"); },
        setItem: () => { throw new Error("storage disabled"); },
        removeItem: () => { throw new Error("storage disabled"); },
      },
    });

    expect(() => captureReferralCode("partner-code")).not.toThrow();
    expect(storedReferralCode()).toBeUndefined();

    if (descriptor) Object.defineProperty(window, "localStorage", descriptor);
  });
});
