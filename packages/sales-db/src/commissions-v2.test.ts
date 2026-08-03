import { randomUUID } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  assertReferredSpendV2Shape,
  isCommissionableSpendV2,
  pendingUsageV2Ref,
  ReferredSpendV2ShapeError,
  type ReferredSpendV2Event,
} from "./commissions-v2.js";
import { computeCommissionChain, type CommissionChainPartner } from "./commissions.js";

function event(overrides: Partial<ReferredSpendV2Event> = {}): ReferredSpendV2Event {
  return {
    commerceEventId: 42n,
    commerceUserId: randomUUID(),
    providerId: "anthropic",
    accountClass: "b2c",
    officialNano: 20_000n,
    chargedNano: 12_000n,
    paidFundedNano: 10_000n,
    bonusFundedNano: 2_000n,
    otherFundedNano: 0n,
    commissionEligible: true,
    releaseGeneration: 7n,
    releaseDigest: "release-v2",
    snapshotDigest: "snapshot-v2",
    occurredAt: new Date("2026-08-01T12:00:00.000Z"),
    ...overrides,
  };
}

function partner(overrides: Partial<CommissionChainPartner> & { partnerId: string }): CommissionChainPartner {
  return { status: "active", commissionBps: 1000, subCommissionBps: 1000, ...overrides };
}

describe("v2 commission chain math (basis = exact paid_funded_nano)", () => {
  it("computes level 0 from the paid basis and level N from the previous level amount", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 1000 }),
      partner({ partnerId: "p1", subCommissionBps: 500 }),
    ], 10_000n);
    expect(entries).toEqual([
      { partnerId: "p0", level: 0, appliedBps: 1000, amountNano: 1_000n },
      { partnerId: "p1", level: 1, appliedBps: 500, amountNano: 50n },
    ]);
  });

  it("floors integer division at every level", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 3333 }),
      partner({ partnerId: "p1", subCommissionBps: 3333 }),
    ], 101n);
    expect(entries.map((entry) => entry.amountNano)).toEqual([33n, 10n]);
  });

  it("never commissions the bonus-funded part of the charge", () => {
    // charged = 12_000, но basis — строго paid 10_000: level 0 = 10% от paid, не от charged.
    const entries = computeCommissionChain([partner({ partnerId: "p0" })], 10_000n);
    expect(entries[0]?.amountNano).toBe(1_000n);
  });

  it("stops at a suspended parent and caps the walk at 10 levels", () => {
    const suspended = computeCommissionChain([
      partner({ partnerId: "p0" }),
      partner({ partnerId: "p1", status: "suspended" }),
      partner({ partnerId: "p2" }),
    ], 1_000_000_000n);
    expect(suspended.map((entry) => entry.partnerId)).toEqual(["p0"]);

    const chain = Array.from({ length: 15 }, (_, index) =>
      partner({ partnerId: `p${index}`, commissionBps: 10000, subCommissionBps: 10000 }));
    const capped = computeCommissionChain(chain, 1_000n);
    expect(capped).toHaveLength(10);
    expect(capped.at(-1)?.level).toBe(9);
  });

  it("stops when a computed amount reaches zero", () => {
    const entries = computeCommissionChain([
      partner({ partnerId: "p0", commissionBps: 1000 }),
      partner({ partnerId: "p1", subCommissionBps: 1 }),
    ], 100n);
    expect(entries).toHaveLength(1);
  });
});

describe("assertReferredSpendV2Shape", () => {
  it("accepts a complete referred-B2C release lineage", () => {
    expect(() => assertReferredSpendV2Shape(event())).not.toThrow();
  });

  it("rejects funding buckets that do not sum to the charged amount", () => {
    expect(() => assertReferredSpendV2Shape(event({ chargedNano: 12_001n })))
      .toThrow(ReferredSpendV2ShapeError);
  });

  it("rejects non-B2C classes, empty lineage identity and negative buckets", () => {
    expect(() => assertReferredSpendV2Shape(event({ accountClass: "b2b" as "b2c" }))).toThrow();
    expect(() => assertReferredSpendV2Shape(event({ providerId: "" }))).toThrow();
    expect(() => assertReferredSpendV2Shape(event({ releaseDigest: "" }))).toThrow();
    expect(() => assertReferredSpendV2Shape(event({ snapshotDigest: "" }))).toThrow();
    expect(() => assertReferredSpendV2Shape(event({ releaseGeneration: 0n }))).toThrow();
    expect(() => assertReferredSpendV2Shape(event({ bonusFundedNano: -1n }))).toThrow();
  });
});

describe("v2 eligibility is fail-closed", () => {
  it("commissions only eligible rows with positive exact paid funding", () => {
    expect(isCommissionableSpendV2(event())).toBe(true);
    expect(isCommissionableSpendV2(event({ commissionEligible: false }))).toBe(false);
    expect(isCommissionableSpendV2(event({
      paidFundedNano: 0n,
      chargedNano: 2_000n, // bonus-only charge
    }))).toBe(false);
  });
});

describe("pendingUsageV2Ref", () => {
  it("is deterministic and distinct per commerce event", () => {
    expect(pendingUsageV2Ref(42n)).toBe("usage-v2:42");
    expect(pendingUsageV2Ref(42n)).toBe(pendingUsageV2Ref(42n));
    expect(pendingUsageV2Ref(43n)).not.toBe(pendingUsageV2Ref(42n));
  });
});
