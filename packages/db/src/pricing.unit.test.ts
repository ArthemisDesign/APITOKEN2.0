import { describe, expect, it } from "vitest";
import { isFreeCreditRef } from "./pricing.js";

// Классификация источника денег движка по engine ref. Реальными (комиссионируемыми) считаются
// ТОЛЬКО депозиты через платёжные провайдеры (`platega:`/`cryptomus:`). Всё остальное — бесплатное.
describe("isFreeCreditRef (whitelist of real-money refs)", () => {
  it("treats real payment-provider deposits as NOT free (they earn commission)", () => {
    expect(isFreeCreditRef("platega:abc-123")).toBe(false);
    expect(isFreeCreditRef("cryptomus:tx_9")).toBe(false);
  });

  it("treats welcome bonus, promo and admin credit as FREE (no commission)", () => {
    expect(isFreeCreditRef("signup-bonus:user-1")).toBe(true);
    expect(isFreeCreditRef("promo:SUMMER")).toBe(true);
    expect(isFreeCreditRef("admin-credit:deadbeef")).toBe(true);
  });

  it("defaults unknown / empty / null refs to FREE so a new money source never over-pays by accident", () => {
    expect(isFreeCreditRef("something-new:1")).toBe(true);
    expect(isFreeCreditRef("")).toBe(true);
    expect(isFreeCreditRef(null)).toBe(true);
    expect(isFreeCreditRef(undefined)).toBe(true);
  });

  it("does not match a provider name that is only a substring (prefix-anchored)", () => {
    expect(isFreeCreditRef("not-platega:1")).toBe(true);
    expect(isFreeCreditRef("xcryptomus:1")).toBe(true);
  });
});
