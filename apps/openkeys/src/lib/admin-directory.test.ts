import { describe, expect, it } from "vitest";
import {
  adminUsagePercent,
  classifyAdminUsage,
  matchesAdminUsage,
  parseAdminUsageFilter,
} from "./admin-directory";

describe("OpenKeys admin usage filters", () => {
  it("keeps unavailable, unused, used, and exhausted keys distinct", () => {
    expect(classifyAdminUsage(null, null)).toBe("unavailable");
    expect(classifyAdminUsage(0n, 50n)).toBe("unused");
    expect(classifyAdminUsage(1n, 49n)).toBe("used");
    expect(classifyAdminUsage(50n, 0n)).toBe("exhausted");
  });

  it("computes a conservative integer usage percent with bigint money", () => {
    expect(adminUsagePercent(null, 100n)).toBeNull();
    expect(adminUsagePercent(0n, 100n)).toBe(0);
    expect(adminUsagePercent(1n, 100n)).toBe(1);
    expect(adminUsagePercent(101n, 100n)).toBe(101);
  });

  it("rejects unknown filters instead of silently widening the directory", () => {
    expect(parseAdminUsageFilter("used")).toBe("used");
    expect(parseAdminUsageFilter("everything")).toBeNull();
    expect(matchesAdminUsage("unused", "all")).toBe(true);
    expect(matchesAdminUsage("unused", "used")).toBe(false);
  });
});
