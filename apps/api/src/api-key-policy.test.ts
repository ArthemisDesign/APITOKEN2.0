import { afterEach, describe, expect, it, vi } from "vitest";
import { createApiKeySchema, updateApiKeyPolicySchema } from "@claude-api/contracts";

afterEach(() => vi.useRealTimers());

describe("API key policy input", () => {
  it("accepts optional limits and expiration while keeping the payload strict", () => {
    expect(createApiKeySchema.parse({
      label: "production",
      spendLimitUsd: "25.50",
      expiresAt: "2099-01-01T00:00:00.000Z",
    })).toEqual({
      label: "production",
      spendLimitUsd: "25.50",
      expiresAt: "2099-01-01T00:00:00.000Z",
    });

    expect(createApiKeySchema.safeParse({ label: "x", unexpected: true }).success).toBe(false);
  });

  it.each(["0", "0.0", "0.00", "-1", "1.234", "9223372036.86", "not-money"])("rejects invalid spend limit %s", (value) => {
    expect(createApiKeySchema.safeParse({ spendLimitUsd: value }).success).toBe(false);
  });

  it("rejects an expiration in the past", () => {
    expect(createApiKeySchema.safeParse({ expiresAt: "2020-01-01T00:00:00.000Z" }).success).toBe(false);
  });

  it("rejects a millisecond-future expiration that floors to the current engine second", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2030-01-01T00:00:00.100Z"));
    expect(createApiKeySchema.safeParse({ expiresAt: "2030-01-01T00:00:00.900Z" }).success).toBe(false);
  });

  it("requires explicit nullable replacement fields and preserves exact nano precision", () => {
    expect(updateApiKeyPolicySchema.parse({
      spendLimitUsd: "1.000000001",
      expiresAt: null,
      totpCode: "123456",
    })).toEqual({ spendLimitUsd: "1.000000001", expiresAt: null, totpCode: "123456" });
    expect(updateApiKeyPolicySchema.parse({
      spendLimitUsd: null,
      expiresAt: "2099-01-01T00:00:00.000Z",
    })).toEqual({ spendLimitUsd: null, expiresAt: "2099-01-01T00:00:00.000Z" });
    expect(updateApiKeyPolicySchema.safeParse({ spendLimitUsd: null }).success).toBe(false);
    expect(updateApiKeyPolicySchema.safeParse({ expiresAt: null }).success).toBe(false);
    expect(updateApiKeyPolicySchema.safeParse({
      spendLimitUsd: "1.0000000001", expiresAt: null,
    }).success).toBe(false);
  });
});
