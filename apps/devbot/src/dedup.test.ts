import { describe, expect, it } from "vitest";
import {
  Dedup,
  FINGERPRINT_TTL_MS,
  STORM_QUIET_MS,
  STORM_THRESHOLD,
  WARNING_MIN_EDIT_INTERVAL_MS,
} from "./dedup.js";
import type { FingerprintEntry } from "./state.js";

const T0 = 1_700_000_000_000;

function makeDedup() {
  const fingerprints: Record<string, FingerprintEntry> = {};
  return { dedup: new Dedup(fingerprints), fingerprints };
}

describe("Dedup fingerprint store", () => {
  it("treats a repeat firing within TTL as the same alert and counts it", () => {
    const { dedup } = makeDedup();
    dedup.register("fp1", { messageId: 10, topic: "warnings", now: T0 });
    const entry = dedup.lookup("fp1", T0 + 1000);
    expect(entry?.count).toBe(1);
    dedup.markRepeat(entry as FingerprintEntry, T0 + 1000);
    expect(dedup.lookup("fp1", T0 + 2000)?.count).toBe(2);
  });

  it("expires fingerprints after 48h TTL", () => {
    const { dedup, fingerprints } = makeDedup();
    dedup.register("fp1", { messageId: 10, topic: "warnings", now: T0 });
    expect(dedup.lookup("fp1", T0 + FINGERPRINT_TTL_MS - 1)).toBeDefined();
    expect(dedup.lookup("fp1", T0 + FINGERPRINT_TTL_MS + 1)).toBeUndefined();
    dedup.prune(T0 + FINGERPRINT_TTL_MS + 1);
    expect(fingerprints.fp1).toBeUndefined();
  });

  it("limits warning edits to one per 5 minutes", () => {
    const { dedup } = makeDedup();
    const entry = dedup.register("fp1", { messageId: 10, topic: "warnings", now: T0 });
    expect(dedup.warningEditAllowed(entry, T0 + 60_000)).toBe(false);
    expect(dedup.warningEditAllowed(entry, T0 + WARNING_MIN_EDIT_INTERVAL_MS + 1)).toBe(true);
    dedup.markEdited(entry, T0 + WARNING_MIN_EDIT_INTERVAL_MS + 1);
    expect(dedup.warningEditAllowed(entry, T0 + WARNING_MIN_EDIT_INTERVAL_MS + 60_000)).toBe(false);
  });
});

describe("Dedup storm coalescing", () => {
  it("suppresses individual criticals after >5 distinct names in 60s", () => {
    const { dedup } = makeDedup();
    for (let i = 1; i <= STORM_THRESHOLD; i += 1) {
      const status = dedup.trackCritical(`Alert${i}`, T0 + i * 1000);
      expect(status.suppressed).toBe(false);
    }
    const trigger = dedup.trackCritical("Alert6", T0 + 6_000);
    expect(trigger.suppressed).toBe(true);
    expect(trigger.started).toBe(true);
    expect(trigger.names).toHaveLength(STORM_THRESHOLD + 1);

    const next = dedup.trackCritical("Alert7", T0 + 7_000);
    expect(next.suppressed).toBe(true);
    expect(next.started).toBe(false);
    expect(next.total).toBeGreaterThan(trigger.total);
  });

  it("does not start a storm for the same alertname repeating", () => {
    const { dedup } = makeDedup();
    for (let i = 0; i < 10; i += 1) {
      expect(dedup.trackCritical("SameAlert", T0 + i * 1000).suppressed).toBe(false);
    }
  });

  it("ends the storm after 10 minutes of quiet", () => {
    const { dedup } = makeDedup();
    for (let i = 1; i <= STORM_THRESHOLD + 1; i += 1) {
      dedup.trackCritical(`Alert${i}`, T0 + i * 1000);
    }
    expect(dedup.trackCritical("Late", T0 + 10_000).suppressed).toBe(true);
    const after = dedup.trackCritical("Fresh", T0 + 10_000 + STORM_QUIET_MS + 1);
    expect(after.suppressed).toBe(false);
  });
});
