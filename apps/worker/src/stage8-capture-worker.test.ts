import assert from "node:assert/strict";
import test from "node:test";
import {
  PricingStage8CaptureJobV2Error,
  Stage8EvidenceV2Error,
  type Stage8CombinedEvidenceV2,
  type Stage8EngineEvidenceV2,
} from "@claude-api/db";
import { EngineClientError } from "@claude-api/engine-client";
import {
  executePricingStage8CaptureAttemptV2,
  stage8CaptureDisposition,
} from "./stage8-capture-worker.service.js";

const digestV2 = `sha256:v2:${"2".repeat(64)}`;

test("durably stores the raw engine artifact before collecting and completing Stage 8", async () => {
  const events: string[] = [];
  const evidence = { evidence_digest: digestV2 } as Stage8EngineEvidenceV2;
  const combined: Stage8CombinedEvidenceV2 = {
    schema_version: 2,
    observed_at: "2026-08-03T00:00:00.000Z",
    valid_until: "2026-08-03T00:05:00.000Z",
    passed: false,
    write_result: "stored",
    source: {
      engine_evidence_digest: digestV2,
      engine_captured_ts: "1785715200",
      engine_window_start_ts: "1785714900",
      engine_window_end_ts: "1785715190",
    },
    releases: {
      target: { generation: "41", commerce_digest: digestV2, engine_digest: digestV2 },
      recovery: { generation: "42", commerce_digest: digestV2, engine_digest: digestV2 },
    },
    inventories: {
      commerce_digest: digestV2,
      engine_digest: digestV2,
      openkeys_digest: digestV2,
      service_digest: digestV2,
    },
    sales_contract_digest: digestV2,
    funding_digest: digestV2,
    shadow_digest: digestV2,
    runtime_floor_digest: digestV2,
    legacy_inflight_count: "7",
    blocker_count: "1",
    blockers: [{ source: "engine", code: "runtime_floor", count: "1", subject_digests: [] }],
    evidence_digest: digestV2,
  };
  let completedRaw = "";

  const result = await executePricingStage8CaptureAttemptV2({
    async capture() {
      events.push("capture");
      return { raw: "{\"source\":\"exact\"}" };
    },
    async persist(raw) {
      events.push(`persist:${raw}`);
      return { artifactId: "artifact-1", evidence };
    },
    async collect(input) {
      events.push(`collect:${input.evidence_digest}`);
      return combined;
    },
    async complete(artifactId, input, rawCombined) {
      events.push(`complete:${artifactId}:${input.passed}`);
      completedRaw = rawCombined;
    },
  });

  assert.equal(result, combined);
  assert.deepEqual(events, [
    "capture",
    'persist:{"source":"exact"}',
    `collect:${digestV2}`,
    "complete:artifact-1:false",
  ]);
  assert.deepEqual(JSON.parse(completedRaw), combined);
  assert.match(completedRaw, /\n$/);
});

test("retries only uncertain transport or local failures and fails closed on protocol evidence", () => {
  assert.equal(stage8CaptureDisposition(new EngineClientError("timeout", undefined, true)), "retry");
  assert.equal(stage8CaptureDisposition(new EngineClientError("malformed", 200, false)), "dead");
  assert.equal(stage8CaptureDisposition(new PricingStage8CaptureJobV2Error("database", false)), "retry");
  assert.equal(stage8CaptureDisposition(new PricingStage8CaptureJobV2Error("collision", true)), "dead");
  assert.equal(stage8CaptureDisposition(new Stage8EvidenceV2Error("digest", "tampered")), "dead");
  assert.equal(stage8CaptureDisposition(new Error("database connection reset")), "retry");
});
