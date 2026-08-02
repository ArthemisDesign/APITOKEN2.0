import JSONbigFactory from "json-bigint";
import { describe, expect, it } from "vitest";
import {
  parseStage8EngineEvidenceV2,
  stage8EngineEvidenceDigestV2,
  type Stage8EngineEvidenceV2,
} from "./multi-discount-stage8-evidence.js";

const JSONbig = JSONbigFactory({ alwaysParseAsBig: true, useNativeBigInt: true });
const digestV2 = `sha256:v2:${"2".repeat(64)}`;
const digestV1 = `sha256:v1:${"1".repeat(64)}`;
const rustGoldenDigest = "sha256:v2:6a06233c292ceb352a64c9e3912e262e9500716190d205986378c9ca9a220ab2";

function fixture(): Stage8EngineEvidenceV2 {
  const report: Stage8EngineEvidenceV2 = {
    schema_version: 2n,
    captured_ts: 2_000n,
    window_start_ts: 1_000n,
    window_end_ts: 1_900n,
    min_samples_per_provider: 1n,
    gemini_client_admissions: 7n,
    passed: true,
    release: {
      target_generation: 101n,
      target_digest: digestV2,
      recovery_generation: 102n,
      recovery_digest: digestV2,
      recovery_link_digest: digestV2,
      inventory_digest: digestV2,
      funding_digest: digestV2,
      target_assignment_count: 1n,
      recovery_assignment_count: 1n,
      active_head: null,
    },
    runtime_manifest: {
      generation: 3n,
      digest: digestV2,
      capabilities: [{ schema_version: 2n, generation: 3n, digest: digestV2 }],
    },
    catalogs: [],
    switches: null,
    counts: {
      total_accounts: 1n,
      active_accounts: 1n,
      account_classes: { b2c: 1n },
      reconciled_accounts: 1n,
      snapshots_by_provider: { anthropic: 1n, google: 1n, openai: 1n },
      evaluations_by_outcome: { resolved: 3n },
      comparisons: { different: 3n },
      scalar_parity_rows: 0n,
      policy_divergence_rows: 3n,
      gemini_usage_rows: 1n,
      gemini_outbox_rows: 1n,
      live_runtime_instances: 2n,
      release_capable_runtime_instances: 2n,
      legacy_inflight_reservations: 0n,
      legacy_inflight_outbox_rows: 0n,
    },
    financial_samples: [{
      subject_digest: digestV1,
      evaluation_digest: digestV2,
      provider_id: "google",
      account_class: "b2c",
      authorized_multiplier_bp: 10_000n,
      payable_multiplier_bp: 5_000n,
      official_hold_nano: 9_223_372_036_854_775_807n,
      legacy_hold_nano: 9_223_372_036_854_775_807n,
      policy_hold_nano: 4_611_686_018_427_387_904n,
      comparison_result: "different",
    }],
    engine_inventory_digest: digestV2,
    funding_digest: digestV2,
    shadow_digest: digestV2,
    runtime_floor_digest: digestV2,
    legacy_inflight_count: 0n,
    blockers: [],
    evidence_digest: `sha256:v2:${"0".repeat(64)}`,
  };
  report.evidence_digest = stage8EngineEvidenceDigestV2(report);
  return report;
}

describe("Stage 8 engine evidence v2 consumer", () => {
  it("preserves signed-i64 nanoUSD and verifies the Rust length-prefixed digest", () => {
    const report = fixture();
    const parsed = parseStage8EngineEvidenceV2(JSONbig.stringify(report));
    expect(parsed.evidence_digest).toBe(rustGoldenDigest);
    expect(parsed.financial_samples[0]!.official_hold_nano).toBe(9_223_372_036_854_775_807n);
  });

  it("rejects a structurally valid report whose evidence digest was replaced", () => {
    const report = fixture();
    const raw = JSONbig.stringify({
      ...report,
      evidence_digest: `sha256:v2:${"f".repeat(64)}`,
    });
    expect(() => parseStage8EngineEvidenceV2(raw)).toThrowError(expect.objectContaining({
      code: "engine_evidence_digest_mismatch",
    }));
  });

  it("keeps legacy inflight as audit evidence without turning a blocker-free report red", () => {
    const report = fixture();
    report.counts.legacy_inflight_reservations = 3n;
    report.counts.legacy_inflight_outbox_rows = 2n;
    report.legacy_inflight_count = 5n;
    report.evidence_digest = stage8EngineEvidenceDigestV2(report);

    const parsed = parseStage8EngineEvidenceV2(JSONbig.stringify(report));
    expect(parsed.passed).toBe(true);
    expect(parsed.legacy_inflight_count).toBe(5n);
  });
});
