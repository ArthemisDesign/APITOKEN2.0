import { describe, expect, it } from "vitest";
import {
  NANO_PER_USD,
  balanceToOfficialNano,
  formatUsd,
  officialBalanceBreakdown,
  officialNanoToBalance,
  usdStringToNano,
} from "./money";

describe("usdStringToNano", () => {
  it("принимает целые доллары", () => {
    expect(usdStringToNano("50")).toBe(50n * NANO_PER_USD);
    expect(usdStringToNano("1")).toBe(NANO_PER_USD);
  });

  it("отвергает всё, что может проскочить как деньги, но ими не является", () => {
    for (const raw of ["0", "-5", "5.5", "5,5", " 5", "5 ", "05", "1e3", "", "abc", "9999999999"]) {
      expect(() => usdStringToNano(raw)).toThrow();
    }
  });
});

describe("formatUsd", () => {
  it("не теряет копейки на больших суммах", () => {
    expect(formatUsd(50n * NANO_PER_USD)).toBe("$50.00");
    expect(formatUsd(1_234_567_891n)).toBe("$1.23");
    expect(formatUsd(999_999_999n)).toBe("$0.99");
  });

  it("умеет округлять до целых для номинала", () => {
    expect(formatUsd(50n * NANO_PER_USD, 0)).toBe("$50");
  });

  it("показывает отрицательный баланс как отрицательный", () => {
    expect(formatUsd(-1n * NANO_PER_USD)).toBe("-$1.00");
  });
});

describe("конвертация номинала и баланса", () => {
  it("при -60% ключ на $50 получает $20 баланса движка", () => {
    expect(officialNanoToBalance(50n * NANO_PER_USD, 4000)).toBe(20n * NANO_PER_USD);
  });

  it("обратный пересчёт возвращает номинал", () => {
    const face = 50n * NANO_PER_USD;
    for (const multBp of [2000, 4000, 5000, 10_000]) {
      expect(balanceToOfficialNano(officialNanoToBalance(face, multBp), multBp)).toBe(face);
    }
  });

  it("нулевой множитель не делит на ноль", () => {
    expect(balanceToOfficialNano(NANO_PER_USD, 0)).toBe(0n);
  });

  it("без скидки номинал равен балансу", () => {
    expect(officialNanoToBalance(7n * NANO_PER_USD, 10_000)).toBe(7n * NANO_PER_USD);
  });

  it("не выдаёт временный резерв за окончательно потраченные деньги", () => {
    const balance = officialBalanceBreakdown(
      12_012_000_000n,
      5_388_000_000n,
      2_600_000_000n,
      4_000,
    );

    expect(balance.available).toBe(30_030_000_000n);
    expect(balance.reserved).toBe(13_470_000_000n);
    expect(balance.remaining).toBe(43_500_000_000n);
    expect(balance.spent).toBe(6_500_000_000n);
    expect(balance.remaining + balance.spent).toBe(50n * NANO_PER_USD);
  });
});
