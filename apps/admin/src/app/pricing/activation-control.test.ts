import { describe, expect, it } from "vitest";
import {
  activationBlockers,
  activationConfirmationError,
  activationConfirmationPhrase,
  type PricingReleaseActivationControlV2,
  type PricingReleaseActivationEvidenceViewV2,
} from "./activation-control";

const DIGEST_A = `sha256:v2:${"a".repeat(64)}`;
const DIGEST_B = `sha256:v2:${"b".repeat(64)}`;
const DIGEST_C = `sha256:v2:${"c".repeat(64)}`;
const DIGEST_D = `sha256:v2:${"d".repeat(64)}`;
const NOW = Date.parse("2026-08-03T00:00:00.000Z");

function evidence(
  overrides: Partial<PricingReleaseActivationEvidenceViewV2> = {},
): PricingReleaseActivationEvidenceViewV2 {
  return {
    evidence_digest: DIGEST_A,
    engine_evidence_digest: DIGEST_B,
    engine_captured_at: "2026-08-02T23:59:30.000Z",
    target_generation: "11",
    target_digest: DIGEST_C,
    recovery_generation: "12",
    recovery_digest: DIGEST_D,
    service_inventory_digest: DIGEST_C,
    legacy_inflight_count: "4",
    blocker_count: "0",
    passed: true,
    observed_at: "2026-08-02T23:59:40.000Z",
    valid_until: "2026-08-03T00:04:40.000Z",
    target_status: "prepared",
    recovery_status: "prepared",
    target_engine_digest: DIGEST_B,
    recovery_engine_digest: DIGEST_C,
    fresh: true,
    source_complete: true,
    local_blockers: [],
    ...overrides,
  };
}

function control(
  overrides: Partial<PricingReleaseActivationControlV2> = {},
): PricingReleaseActivationControlV2 {
  return {
    database_observed_at: "2026-08-03T00:00:00.000Z",
    unresolved_pricing_jobs: 0,
    engine: {
      observed_at: "2026-08-03T00:00:00.000Z",
      available: true,
      head: null,
    },
    releases: [],
    evidence: [],
    jobs: [],
    receipts: [],
    ...overrides,
  };
}

describe("pricing release activation control", () => {
  it("allows cutover only with a clear fresh snapshot and absent head", () => {
    expect(activationBlockers(control(), evidence(), "cutover", NOW)).toEqual([]);
    expect(activationBlockers(control({
      unresolved_pricing_jobs: 2,
      engine: { observed_at: "2026-08-03T00:00:00.000Z", available: false, head: null },
    }), evidence({
      passed: false,
      source_complete: false,
      local_blockers: ["service_inventory_identity_missing"],
    }), "cutover", NOW, true)).toEqual([
      "activation_control_refresh_failed",
      "engine_unavailable",
      "evidence_not_passed",
      "source_incomplete",
      "unresolved_pricing_jobs",
      "authority:service_inventory_identity_missing",
    ]);
  });

  it("expires evidence against current browser time even if the loaded fresh flag is stale", () => {
    expect(activationBlockers(control(), evidence(), "cutover", Date.parse("2026-08-03T00:05:00.000Z")))
      .toContain("evidence_expired");
  });

  it("requires exact target head and durable cutover receipt before recovery", () => {
    const selected = evidence();
    const active = control({
      engine: {
        observed_at: "2026-08-03T00:00:00.000Z",
        available: true,
        head: { active_generation: 11, active_digest: DIGEST_B, head_version: 1, updated_ts: 1 },
      },
    });
    expect(activationBlockers(active, selected, "recovery", NOW)).toEqual(["cutover_receipt_missing"]);
    active.receipts.push({
      activation_id: "1",
      activation_kind: "cutover",
      release_generation: "11",
      release_digest: DIGEST_C,
      evidence_digest: DIGEST_A,
      head_version: "1",
      receipt_digest: DIGEST_C,
      activated_at: "2026-08-03T00:00:00.000Z",
      created_at: "2026-08-03T00:00:00.000Z",
    });
    expect(activationBlockers(active, selected, "recovery", NOW)).toEqual([]);
  });

  it("binds confirmation to kind, generation and exact evidence suffix", () => {
    const selected = evidence();
    const phrase = activationConfirmationPhrase("cutover", selected);
    expect(phrase).toBe("CUTOVER 11 aaaaaaaa");
    expect(activationConfirmationError({ reason: "reviewed full inventory", confirmation: phrase }, phrase)).toBeNull();
    expect(activationConfirmationError({ reason: "short", confirmation: phrase }, phrase)).toContain("10 символов");
    expect(activationConfirmationError({ reason: "reviewed full inventory", confirmation: "CUTOVER 11 wrong" }, phrase))
      .toContain(phrase);
  });
});
