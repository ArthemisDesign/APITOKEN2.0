import { describe, expect, it } from "vitest";
import type { OpenkeysConfig } from "./config";
import { issueUsageSession, usageSessionToken, validViewToken } from "./usage-session";

const config = {
  sessionSecret: "0123456789abcdef0123456789abcdef",
} as OpenkeysConfig;
const token = "abcdefghijklmnopqrstuv";

describe("usage session", () => {
  it("accepts a signed unexpired view token", () => {
    expect(usageSessionToken(issueUsageSession(token, config), config)).toBe(token);
  });

  it("rejects token, expiry, and signature tampering", () => {
    const parts = issueUsageSession(token, config).split(".");
    expect(usageSessionToken([`${token.slice(0, -1)}w`, parts[1], parts[2]].join("."), config)).toBeNull();
    expect(usageSessionToken([parts[0], "1", parts[2]].join("."), config)).toBeNull();
    expect(usageSessionToken([parts[0], parts[1], "tampered"].join("."), config)).toBeNull();
  });

  it("validates the exact public capability shape", () => {
    expect(validViewToken(token)).toBe(true);
    expect(validViewToken(`${token}x`)).toBe(false);
    expect(validViewToken("../invalid-token-value")).toBe(false);
  });
});
