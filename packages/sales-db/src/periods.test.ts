import { describe, expect, it } from "vitest";
import {
  lastEndedPeriod,
  lockUntil,
  nextPeriod,
  periodFor,
  periodInPayoutWindow,
  phaseOf,
  previousPeriod,
  windowEnd,
  windowStart,
} from "./periods.js";

const iso = (d: Date) => d.toISOString();

describe("periodFor", () => {
  it("maps early days to P1", () => {
    const p = periodFor(new Date("2026-07-01T00:00:00Z"));
    expect(p.index).toBe(1);
    expect(p.key).toBe("2026-07-P1");
    expect(iso(p.start)).toBe("2026-07-01T00:00:00.000Z");
    expect(iso(p.end)).toBe("2026-07-16T00:00:00.000Z");
  });

  it("day 15 is still P1, day 16 is P2", () => {
    expect(periodFor(new Date("2026-07-15T23:59:59Z")).index).toBe(1);
    const p2 = periodFor(new Date("2026-07-16T00:00:00Z"));
    expect(p2.index).toBe(2);
    expect(iso(p2.start)).toBe("2026-07-16T00:00:00.000Z");
    expect(iso(p2.end)).toBe("2026-08-01T00:00:00.000Z");
  });

  it("handles end of month and February", () => {
    expect(periodFor(new Date("2026-02-28T12:00:00Z")).key).toBe("2026-02-P2");
    expect(iso(periodFor(new Date("2026-02-16T00:00:00Z")).end)).toBe("2026-03-01T00:00:00.000Z");
  });
});

describe("next/previous period", () => {
  it("P1 -> P2 -> P1(next month)", () => {
    const p1 = periodFor(new Date("2026-07-05T00:00:00Z"));
    const p2 = nextPeriod(p1);
    expect(p2.key).toBe("2026-07-P2");
    const p3 = nextPeriod(p2);
    expect(p3.key).toBe("2026-08-P1");
  });

  it("crosses year boundary December P2 -> January P1", () => {
    const dec2 = periodFor(new Date("2026-12-20T00:00:00Z"));
    expect(dec2.key).toBe("2026-12-P2");
    expect(nextPeriod(dec2).key).toBe("2027-01-P1");
    expect(previousPeriod(periodFor(new Date("2027-01-03T00:00:00Z"))).key).toBe("2026-12-P2");
  });

  it("previous is the inverse of next", () => {
    for (const seed of ["2026-01-10", "2026-06-20", "2026-12-16"]) {
      const p = periodFor(new Date(`${seed}T00:00:00Z`));
      expect(previousPeriod(nextPeriod(p)).key).toBe(p.key);
      expect(nextPeriod(previousPeriod(p)).key).toBe(p.key);
    }
  });
});

describe("lock and window", () => {
  it("P1 (ends Jul 16): lock 7d, window 3d", () => {
    const p1 = periodFor(new Date("2026-07-10T00:00:00Z"));
    expect(iso(lockUntil(p1))).toBe("2026-07-23T00:00:00.000Z");
    expect(iso(windowStart(p1))).toBe("2026-07-23T00:00:00.000Z");
    expect(iso(windowEnd(p1))).toBe("2026-07-26T00:00:00.000Z");
  });

  it("phase transitions across the lifecycle", () => {
    const p = periodFor(new Date("2026-07-10T00:00:00Z")); // ends 07-16
    expect(phaseOf(p, new Date("2026-07-14T00:00:00Z"))).toBe("accruing");
    expect(phaseOf(p, new Date("2026-07-16T00:00:00Z"))).toBe("locked");
    expect(phaseOf(p, new Date("2026-07-22T23:59:59Z"))).toBe("locked");
    expect(phaseOf(p, new Date("2026-07-23T00:00:00Z"))).toBe("payable");
    expect(phaseOf(p, new Date("2026-07-25T23:59:59Z"))).toBe("payable");
    expect(phaseOf(p, new Date("2026-07-26T00:00:00Z"))).toBe("closed");
  });
});

describe("payout window helpers", () => {
  it("periodInPayoutWindow finds the payable period", () => {
    // On Jul 24, P1 (ended Jul 16) is in its payout window (Jul 23–26).
    const win = periodInPayoutWindow(new Date("2026-07-24T00:00:00Z"));
    expect(win?.key).toBe("2026-07-P1");
  });

  it("returns null when no window is open", () => {
    expect(periodInPayoutWindow(new Date("2026-07-18T00:00:00Z"))).toBeNull();
  });

  it("lastEndedPeriod is the previous period", () => {
    expect(lastEndedPeriod(new Date("2026-07-20T00:00:00Z")).key).toBe("2026-07-P1");
    expect(lastEndedPeriod(new Date("2026-07-05T00:00:00Z")).key).toBe("2026-06-P2");
  });
});
