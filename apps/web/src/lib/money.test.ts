import { describe, expect, it } from "vitest";
import { nanoToUsd, normalizeUsd, wholeUsdError } from "./money";

describe("money formatting", () => {
  it("formats nanodollars without floating point conversion", () => {
    expect(nanoToUsd("25000000000")).toBe("$25");
    expect(nanoToUsd("1234567890")).toBe("$1.23");
    expect(nanoToUsd("-500000000")).toBe("-$0.5");
  });

  it("normalizes backend USD strings for display", () => {
    expect(normalizeUsd("$00025.500000000")).toBe("$25.5");
    expect(normalizeUsd("1000.00")).toBe("$1,000");
  });

  it("accepts any positive whole USD amount and rejects malformed values", () => {
    expect(wholeUsdError("1")).toBeNull();
    expect(wholeUsdError("10000")).toBeNull();
    // No client-side ceiling: arbitrary positive whole USD is allowed; the backend
    // enforces the authoritative min/max (C77).
    expect(wholeUsdError("10001")).toBeNull();
    expect(wholeUsdError("999999")).toBeNull();
    expect(wholeUsdError("0")).toMatch(/positive whole/);
    expect(wholeUsdError("01")).toMatch(/positive whole/);
    expect(wholeUsdError("1.5")).toMatch(/positive whole/);
  });
});
