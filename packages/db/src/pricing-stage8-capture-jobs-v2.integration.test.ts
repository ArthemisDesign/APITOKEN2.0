import { randomUUID } from "node:crypto";
import type { PricingStage8CaptureRequestV2 } from "@claude-api/contracts";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import JSONbigFactory from "json-bigint";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  claimNextPricingStage8CaptureJobV2,
  completePricingStage8CaptureJobV2,
  createDatabase,
  persistPricingStage8EngineArtifactV2,
  readPricingStage8CaptureControlV2,
  recoverStalePricingStage8CaptureJobsV2,
  releasePricingStage8CaptureJobV2,
  stagePricingStage8CaptureJobV2,
  stage5V2Digest,
  stage8EngineEvidenceDigestV2,
  type Database,
  type Stage8CombinedEvidenceV2,
  type Stage8EngineEvidenceV2,
  type StagePricingStage8CaptureJobV2Input,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const JSONbig = JSONbigFactory({ alwaysParseAsBig: true, useNativeBigInt: true });
const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function digest(label: string): string {
  return stage5V2Digest("stage8-managed-capture-integration", label);
}

function request(): PricingStage8CaptureRequestV2 {
  const windowEnd = Math.floor(Date.now() / 1_000) - 10;
  return {
    target_generation: 91_001,
    recovery_generation: 91_002,
    window_start_ts: windowEnd - 90,
    window_end_ts: windowEnd,
    min_samples_per_provider: 1,
    financial_sample_size: 20,
    gemini_client_admissions: 1,
  };
}

function stageInput(
  idempotencyKey = randomUUID(),
  captureRequest = request(),
): StagePricingStage8CaptureJobV2Input {
  return {
    idempotencyKey,
    request: captureRequest,
    operatorId: "pricing-operator@example.test",
    reason: "capture fresh full-inventory Stage 8 evidence",
  };
}

function engineEvidence(captureRequest: PricingStage8CaptureRequestV2): Stage8EngineEvidenceV2 {
  const captured = BigInt(captureRequest.window_end_ts + 1);
  const report: Stage8EngineEvidenceV2 = {
    schema_version: 2n,
    captured_ts: captured,
    window_start_ts: BigInt(captureRequest.window_start_ts),
    window_end_ts: BigInt(captureRequest.window_end_ts),
    min_samples_per_provider: BigInt(captureRequest.min_samples_per_provider),
    gemini_client_admissions: BigInt(captureRequest.gemini_client_admissions),
    passed: true,
    release: {
      target_generation: BigInt(captureRequest.target_generation),
      target_digest: digest("target"),
      recovery_generation: BigInt(captureRequest.recovery_generation),
      recovery_digest: digest("recovery"),
      recovery_link_digest: digest("recovery-link"),
      inventory_digest: digest("engine-inventory"),
      funding_digest: digest("funding"),
      target_assignment_count: 4n,
      recovery_assignment_count: 4n,
      active_head: null,
    },
    runtime_manifest: {
      generation: 2n,
      digest: digest("runtime-manifest"),
      capabilities: [{
        schema_version: 2n,
        generation: 2n,
        digest: digest("runtime-capability"),
      }],
    },
    catalogs: [],
    switches: null,
    counts: {
      total_accounts: 4n,
      active_accounts: 4n,
      account_classes: { b2c: 1n, b2b: 1n, openkeys: 1n, service: 1n },
      reconciled_accounts: 4n,
      snapshots_by_provider: { anthropic: 4n, openai: 4n, google: 4n },
      evaluations_by_outcome: { resolved: 12n },
      comparisons: { different: 12n },
      scalar_parity_rows: 0n,
      policy_divergence_rows: 12n,
      gemini_usage_rows: 1n,
      gemini_outbox_rows: 1n,
      live_runtime_instances: 2n,
      release_capable_runtime_instances: 2n,
      legacy_inflight_reservations: 0n,
      legacy_inflight_outbox_rows: 0n,
    },
    financial_samples: [],
    engine_inventory_digest: digest("engine-inventory"),
    funding_digest: digest("funding"),
    shadow_digest: digest("shadow"),
    runtime_floor_digest: digest("runtime-floor"),
    legacy_inflight_count: 0n,
    blockers: [],
    evidence_digest: digest("placeholder"),
  };
  report.evidence_digest = stage8EngineEvidenceDigestV2(report);
  return report;
}

function blockedCombinedEvidence(
  captureRequest: PricingStage8CaptureRequestV2,
  evidence: Stage8EngineEvidenceV2,
): Stage8CombinedEvidenceV2 {
  const observed = new Date();
  const report: Stage8CombinedEvidenceV2 = {
    schema_version: 2,
    observed_at: observed.toISOString(),
    valid_until: new Date(observed.getTime() + 300_000).toISOString(),
    passed: false,
    write_result: "not_persisted",
    source: {
      engine_evidence_digest: evidence.evidence_digest,
      engine_captured_ts: evidence.captured_ts.toString(),
      engine_window_start_ts: evidence.window_start_ts.toString(),
      engine_window_end_ts: evidence.window_end_ts.toString(),
    },
    releases: {
      target: {
        generation: String(captureRequest.target_generation),
        commerce_digest: digest("commerce-target"),
        engine_digest: evidence.release.target_digest,
      },
      recovery: {
        generation: String(captureRequest.recovery_generation),
        commerce_digest: digest("commerce-recovery"),
        engine_digest: evidence.release.recovery_digest,
      },
    },
    inventories: {
      commerce_digest: digest("commerce-inventory"),
      engine_digest: evidence.engine_inventory_digest,
      openkeys_digest: digest("openkeys-inventory"),
      service_digest: digest("service-inventory"),
    },
    sales_contract_digest: digest("sales-contract"),
    funding_digest: evidence.funding_digest,
    shadow_digest: evidence.shadow_digest,
    runtime_floor_digest: evidence.runtime_floor_digest,
    legacy_inflight_count: evidence.legacy_inflight_count.toString(),
    blocker_count: "1",
    blockers: [{
      source: "commerce",
      code: "fresh_inventory_drift",
      count: "1",
      subject_digests: [digest("fresh-inventory-drift")],
    }],
    evidence_digest: digest("placeholder-combined"),
  };
  const {
    write_result: _writeResult,
    evidence_digest: _evidenceDigest,
    ...identity
  } = report;
  report.evidence_digest = stage5V2Digest("stage8-combined-evidence", identity);
  return report;
}

describe.runIf(Boolean(connectionString))("managed Stage 8 capture jobs", () => {
  let admin: Client;
  let seed: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `stage8capture_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seed = new Client({ connectionString: url.toString() });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "stage8-managed-capture-test");
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database?.pool.end();
    await seed?.end();
    if (admin) {
      await admin.query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        [databaseName],
      );
      await admin.query(`DROP DATABASE IF EXISTS ${quoteIdentifier(databaseName)}`);
      await admin.end();
    }
  }, TEST_TIMEOUT_MS);

  beforeEach(async () => {
    const tables = await seed.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    await seed.query(
      `TRUNCATE TABLE ${tables.rows.map((row) => quoteIdentifier(row.tablename)).join(", ")} RESTART IDENTITY CASCADE`,
    );
  });

  it("stages one exact idempotent request and never creates activation authority", async () => {
    const input = stageInput();
    const first = await stagePricingStage8CaptureJobV2(database, input);
    const replay = await stagePricingStage8CaptureJobV2(database, structuredClone(input));
    expect(replay).toEqual(first);

    await expect(stagePricingStage8CaptureJobV2(database, {
      ...input,
      request: { ...input.request, gemini_client_admissions: 2 },
    })).rejects.toMatchObject({
      permanent: true,
      message: "Stage 8 capture idempotency key has a different immutable request",
    });

    const stored = await seed.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_stage8_capture_jobs_v2) AS capture_jobs,
        (SELECT count(*)::int FROM audit_log
          WHERE action = 'pricing_stage8_capture_staged') AS capture_audits,
        (SELECT count(*)::int FROM pricing_release_control_jobs_v2
          WHERE job_kind IN ('activate_release', 'activate_recovery')) AS activation_jobs,
        (SELECT count(*)::int FROM pricing_release_activation_receipts_v2) AS activation_receipts
    `);
    expect(stored.rows[0]).toEqual({
      capture_jobs: 1,
      capture_audits: 1,
      activation_jobs: 0,
      activation_receipts: 0,
    });
  });

  it("persists exact engine bytes before atomically completing a blocked capture", async () => {
    const input = stageInput();
    const staged = await stagePricingStage8CaptureJobV2(database, input);
    const job = await claimNextPricingStage8CaptureJobV2(database, "worker-a", 300_000, 10);
    expect(job).toMatchObject({ id: staged.jobId, attempts: 1, request: input.request });
    await expect(claimNextPricingStage8CaptureJobV2(database, "worker-b", 300_000, 10))
      .resolves.toBeNull();

    const evidence = engineEvidence(input.request);
    const rawEngine = `${JSONbig.stringify(evidence, null, 2)}\n`;
    const artifact = await persistPricingStage8EngineArtifactV2(
      database,
      job!,
      "worker-a",
      rawEngine,
    );
    await expect(persistPricingStage8EngineArtifactV2(database, job!, "worker-a", rawEngine))
      .resolves.toEqual(artifact);
    const durableSource = await seed.query<{
      engine_payload_json: string;
      combined_payload_json: string | null;
    }>(`
      SELECT engine_payload_json, combined_payload_json
      FROM pricing_stage8_capture_artifacts_v2
      WHERE id = $1
    `, [artifact.artifactId]);
    expect(durableSource.rows[0]).toEqual({
      engine_payload_json: rawEngine,
      combined_payload_json: null,
    });

    const combined = blockedCombinedEvidence(input.request, artifact.evidence);
    const rawCombined = `${JSON.stringify(combined, null, 2)}\n`;
    const tampered = { ...combined, blocker_count: "2" };
    await expect(completePricingStage8CaptureJobV2(
      database,
      job!,
      "worker-a",
      artifact.artifactId,
      tampered,
      `${JSON.stringify(tampered, null, 2)}\n`,
    )).rejects.toMatchObject({
      permanent: true,
      message: "combined Stage 8 artifact differs from its engine source or durable request",
    });
    await completePricingStage8CaptureJobV2(
      database,
      job!,
      "worker-a",
      artifact.artifactId,
      combined,
      rawCombined,
    );

    const control = await readPricingStage8CaptureControlV2(database, 1);
    expect(control.countsByStatus).toEqual({
      pending: 0,
      processing: 0,
      retry: 0,
      passed: 0,
      blocked: 1,
      dead: 0,
    });
    expect(control.jobs[0]).toMatchObject({
      id: staged.jobId,
      status: "blocked",
      attempts: 1,
      resultEngineEvidenceDigest: evidence.evidence_digest,
      resultCombinedEvidenceDigest: combined.evidence_digest,
      resultPassed: false,
    });
    expect(control.artifacts[0]).toMatchObject({
      id: artifact.artifactId,
      combinedEvidenceDigest: combined.evidence_digest,
      combinedPassed: false,
      combinedWriteResult: "not_persisted",
      combinedBlockerCount: "1",
      combinedBlockers: combined.blockers,
      combinedBlockersTruncated: false,
    });
    expect(control.artifacts[0]!.combinedObservedAt?.toISOString()).toBe(combined.observed_at);
    expect(control.artifacts[0]!.combinedValidUntil?.toISOString()).toBe(combined.valid_until);
    const durableResult = await seed.query(`
      SELECT
        (SELECT combined_payload_json FROM pricing_stage8_capture_artifacts_v2
          WHERE id = $1) AS combined_payload_json,
        (SELECT count(*)::int FROM pricing_release_control_jobs_v2
          WHERE job_kind IN ('activate_release', 'activate_recovery')) AS activation_jobs
    `, [artifact.artifactId]);
    expect(durableResult.rows[0]).toEqual({
      combined_payload_json: rawCombined,
      activation_jobs: 0,
    });
  });

  it("recovers retryable leases and fails closed at the configured attempt bound", async () => {
    const retryInput = stageInput();
    await stagePricingStage8CaptureJobV2(database, retryInput);
    const first = await claimNextPricingStage8CaptureJobV2(database, "worker-a", 300_000, 2);
    expect(first?.attempts).toBe(1);
    await seed.query(`
      UPDATE pricing_stage8_capture_jobs_v2
      SET locked_at = now() - interval '1 hour'
      WHERE id = $1
    `, [first!.id]);
    await expect(recoverStalePricingStage8CaptureJobsV2(database, 1, 2)).resolves.toBe(1);

    const second = await claimNextPricingStage8CaptureJobV2(database, "worker-b", 300_000, 2);
    expect(second).toMatchObject({ id: first!.id, attempts: 2 });
    await expect(releasePricingStage8CaptureJobV2(
      database,
      second!,
      "worker-b",
      "retry",
      "upstream transport remained uncertain",
      1,
      2,
    )).resolves.toBe("dead");

    const staleInput = stageInput();
    await stagePricingStage8CaptureJobV2(database, staleInput);
    const stale = await claimNextPricingStage8CaptureJobV2(database, "worker-c", 300_000, 1);
    await seed.query(`
      UPDATE pricing_stage8_capture_jobs_v2
      SET locked_at = now() - interval '1 hour'
      WHERE id = $1
    `, [stale!.id]);
    await expect(recoverStalePricingStage8CaptureJobsV2(database, 1, 1)).resolves.toBe(1);

    const control = await readPricingStage8CaptureControlV2(database);
    expect(control.countsByStatus.dead).toBe(2);
    expect(control.jobs).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: first!.id, status: "dead", attempts: 2 }),
      expect.objectContaining({
        id: stale!.id,
        status: "dead",
        attempts: 1,
        lastError: "Stage 8 capture lease expired at the maximum attempt count",
      }),
    ]));
    await expect(claimNextPricingStage8CaptureJobV2(database, "worker-d", 300_000, 2))
      .resolves.toBeNull();
  });
});
