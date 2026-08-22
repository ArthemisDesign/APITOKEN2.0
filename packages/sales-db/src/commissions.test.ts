import { describe, expect, it } from "vitest";
import {
  computeCommissionChain,
  computeConservedCommissionChain,
  type CommissionChainPartner,
} from "./commissions.js";

function partner(overrides: Partial<CommissionChainPartner> & { partnerId: string }): CommissionChainPartner {
  return { status: "active", commissionBps: 1000, subCommissionBps: 1000, ...overrides };
}

describe("computeCommissionChain", () => {
  it("computes a level-0 entry with the direct partner's commission_bps", () => {
    const entries = computeCommissionChain([partner({ partnerId: "p0", commissionBps: 1500 })], 1_000_000_000n);
    expect(entries).toEqual([
      { partnerId: "p0", level: 0, appliedBps: 1500, amountNano: 150_000_000n },
    ]);
  });

  it("walks up the chain applying each parent's sub_commission_bps to the previous level's amount", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 1000 }),
      partner({ partnerId: "p1", subCommissionBps: 2000 }),
      partner({ partnerId: "p2", subCommissionBps: 500 }),
    ], 10_000_000_000n);
    expect(entries).toEqual([
      { partnerId: "p0", level: 0, appliedBps: 1000, amountNano: 1_000_000_000n },
      { partnerId: "p1", level: 1, appliedBps: 2000, amountNano: 200_000_000n },
      { partnerId: "p2", level: 2, appliedBps: 500, amountNano: 10_000_000n },
    ]);
  });

  it("uses the exact child edge instead of the parent's legacy global override", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 1000, parentOverrideBps: 1750 }),
      partner({ partnerId: "p1", subCommissionBps: 400 }),
    ], 10_000_000n);
    expect(entries).toEqual([
      { partnerId: "p0", level: 0, appliedBps: 1000, amountNano: 1_000_000n },
      { partnerId: "p1", level: 1, appliedBps: 1750, amountNano: 175_000n },
    ]);
  });

  it("keeps the parent's legacy fallback for an edge with no explicit override", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 1000, parentOverrideBps: null }),
      partner({ partnerId: "p1", subCommissionBps: 650 }),
    ], 10_000_000n);
    expect(entries[1]?.appliedBps).toBe(650);
  });

  it("floors integer division at every level", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 3333 }),
      partner({ partnerId: "p1", subCommissionBps: 3333 }),
    ], 101n);
    // 101 * 3333 / 10000 = 33.66... -> 33; 33 * 3333 / 10000 = 10.99... -> 10
    expect(entries.map((entry) => entry.amountNano)).toEqual([33n, 10n]);
  });

  it("stops when a computed amount reaches zero", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 1000 }),
      partner({ partnerId: "p1", subCommissionBps: 1 }),
      partner({ partnerId: "p2", subCommissionBps: 10000 }),
    ], 100n);
    // level0 = 10; level1 = 10 * 1 / 10000 = 0 -> stop
    expect(entries).toHaveLength(1);
  });

  it("stops at a suspended partner without creating its entry", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0" }),
      partner({ partnerId: "p1", status: "suspended" }),
      partner({ partnerId: "p2" }),
    ], 1_000_000_000n);
    expect(entries.map((entry) => entry.partnerId)).toEqual(["p0"]);
  });

  it("produces no entries when the direct partner is suspended", () => {
    const entries = computeCommissionChain([partner({ partnerId: "p0", status: "suspended" })], 1_000_000_000n);
    expect(entries).toEqual([]);
  });

  it("produces no entries for a non-positive amount", () => {
    expect(computeCommissionChain([partner({ partnerId: "p0" })], 0n)).toEqual([]);
    expect(computeCommissionChain([partner({ partnerId: "p0" })], -5n)).toEqual([]);
  });

  it("caps the walk at exactly 10 levels (0..9)", () => {
    const chain = Array.from({ length: 15 }, (_, index) =>
      partner({ partnerId: `p${index}`, commissionBps: 10000, subCommissionBps: 10000 }));
    const entries = computeCommissionChain(chain, 1_000n);
    expect(entries).toHaveLength(10);
    expect(entries.at(-1)?.level).toBe(9);
  });

  it("stops at a pending partner (only active partners earn)", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0" }),
      partner({ partnerId: "p1", status: "pending" }),
      partner({ partnerId: "p2" }),
    ], 1_000_000_000n);
    expect(entries.map((entry) => entry.partnerId)).toEqual(["p0"]);
  });

  it("produces no entries when the direct partner is pending", () => {
    const entries = computeCommissionChain([partner({ partnerId: "p0", status: "pending" })], 1_000_000_000n);
    expect(entries).toEqual([]);
  });
});

describe("computeConservedCommissionChain", () => {
  const occurredAt = new Date("2026-08-22T12:00:00.000Z");
  const member = (overrides: Partial<CommissionChainPartner> & { partnerId: string }) => partner({
    programEnabled: true,
    programStartedAt: new Date("2026-08-01T00:00:00.000Z"),
    ...overrides,
  });

  it("withholds every parent cut from the fixed direct commission pool", () => {
    const entries = computeConservedCommissionChain([
      member({ partnerId: "child", commissionBps: 1000, parentOverrideBps: 2000 }),
      member({ partnerId: "parent", parentOverrideBps: 1000 }),
      member({ partnerId: "root" }),
    ], 1_000_000n, occurredAt);

    expect(entries).toEqual([
      {
        partnerId: "child", level: 0, appliedBps: 1000,
        grossAmountNano: 100_000n, withheldAmountNano: 20_000n, amountNano: 80_000n,
      },
      {
        partnerId: "parent", level: 1, appliedBps: 2000,
        grossAmountNano: 20_000n, withheldAmountNano: 2_000n, amountNano: 18_000n,
      },
      {
        partnerId: "root", level: 2, appliedBps: 1000,
        grossAmountNano: 2_000n, withheldAmountNano: 0n, amountNano: 2_000n,
      },
    ]);
    expect(entries.reduce((sum, entry) => sum + entry.amountNano, 0n)).toBe(100_000n);
  });

  it("does not withhold when the parent is disabled, suspended or not started yet", () => {
    for (const ineligibleParent of [
      member({ partnerId: "disabled", programEnabled: false }),
      member({ partnerId: "suspended", status: "suspended" }),
      member({ partnerId: "late", programStartedAt: new Date("2026-08-23T00:00:00.000Z") }),
    ]) {
      expect(computeConservedCommissionChain([
        member({ partnerId: "child", commissionBps: 1000, parentOverrideBps: 2000 }),
        ineligibleParent,
      ], 1_000_000n, occurredAt)).toEqual([{
        partnerId: "child", level: 0, appliedBps: 1000,
        grossAmountNano: 100_000n, withheldAmountNano: 0n, amountNano: 100_000n,
      }]);
    }
  });

  it("creates no new-program commission for a legacy or late direct membership", () => {
    expect(computeConservedCommissionChain([
      partner({ partnerId: "legacy" }),
    ], 1_000_000n, occurredAt)).toEqual([]);
    expect(computeConservedCommissionChain([
      member({ partnerId: "late", programStartedAt: new Date("2026-08-23T00:00:00.000Z") }),
    ], 1_000_000n, occurredAt)).toEqual([]);
  });

  it("rejects an invalid Team edge before any ledger write", () => {
    expect(() => computeConservedCommissionChain([
      member({ partnerId: "child", parentOverrideBps: 2001 }),
      member({ partnerId: "parent" }),
    ], 1_000_000n, occurredAt)).toThrow("between 0 and 2000 bps");
  });
});
