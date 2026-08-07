import { randomUUID } from "node:crypto";
import { Client } from "pg";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import {
  advancePricingReleaseOrchestrationV2,
  createDatabase,
  readPricingReleaseOrchestrationControlV2,
  stagePricingReleaseOrchestrationV2,
  type Database,
  type PricingReleaseOrchestrationReadersV2,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function engineWithHead(head: { active_generation: number; active_digest: string } | null) {
  return {
    getPricingReleaseHeadV2: async () => head === null
      ? null
      : {
        active_generation: head.active_generation,
        active_digest: head.active_digest,
        head_version: 2,
        updated_ts: Math.floor(Date.now() / 1_000),
      },
  } as unknown as PricingReleaseOrchestrationReadersV2["engine"];
}

describe.runIf(Boolean(connectionString))("pricing release orchestration v2", () => {
  let admin: Client;
  let seed: Client;
  let databaseName: string;
  let databaseUrl: string;
  let database: Database;

  const readers: PricingReleaseOrchestrationReadersV2 = {
    engine: engineWithHead(null),
    openkeys: {
      getPage: async () => ({
        inventory_digest: "sha256:v2:" + "0".repeat(64),
        accounts: [],
        next_after_account_id: null,
      }),
    },
  };

  beforeAll(async () => {
    databaseName = `orch_v2_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    databaseUrl = url.toString();
    seed = new Client({ connectionString: databaseUrl });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(databaseUrl, "orchestration-test");
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

  async function stageIntent(capabilityGeneration = 6) {
    return stagePricingReleaseOrchestrationV2(database, {
      idempotencyKey: randomUUID(),
      capabilityGeneration,
      operatorId: "pricing-control-worker:integration",
      reason: "drive the successor pair live",
    });
  }

  it("stages one intent idempotently and admits at most one active orchestration", async () => {
    const key = randomUUID();
    const first = await stagePricingReleaseOrchestrationV2(database, {
      idempotencyKey: key,
      capabilityGeneration: 6,
      operatorId: "pricing-control-worker:integration",
      reason: "drive the successor pair live",
    });
    expect(first.idempotentReplay).toBe(false);
    const replay = await stagePricingReleaseOrchestrationV2(database, {
      idempotencyKey: key,
      capabilityGeneration: 6,
      operatorId: "pricing-control-worker:integration",
      reason: "drive the successor pair live",
    });
    expect(replay).toEqual({ orchestrationId: first.orchestrationId, idempotentReplay: true });
    await expect(stagePricingReleaseOrchestrationV2(database, {
      idempotencyKey: key,
      capabilityGeneration: 7,
      operatorId: "pricing-control-worker:integration",
      reason: "drive the successor pair live",
    })).rejects.toThrow(/different immutable payload/);
    await expect(stageIntent(7)).rejects.toThrow(/already active/);
    const control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations).toHaveLength(1);
    expect(control.orchestrations[0]).toMatchObject({
      step: "materialize_pair",
      status: "active",
      cycle: 1,
      capability_generation: "6",
    });
  });

  it("re-cycles a drift-blocked capture into a fresh pair and fails closed otherwise", async () => {
    const staged = await stageIntent();
    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'capture', target_generation = 41, recovery_generation = 42
      WHERE id = $1
    `, [staged.orchestrationId]);
    const captureJob = async (status: string) => seed.query(`
      INSERT INTO pricing_stage8_capture_jobs_v2 (
        idempotency_key, request_digest, target_generation, recovery_generation,
        window_start_at, window_end_at, min_samples_per_provider, financial_sample_size,
        gemini_client_admissions, operator_id, reason, status, completed_at,
        result_engine_evidence_digest, result_combined_evidence_digest, result_passed
      ) VALUES ($1, $2, 41, 42, now() - interval '20 minutes', now() - interval '5 minutes',
                8, 100, 0, 'pricing-control-worker:integration', 'drive the successor pair live', $3,
                now(), $4, $5, false)
    `, [
      randomUUID(),
      "sha256:v2:" + "1".repeat(64),
      status,
      "sha256:v2:" + "2".repeat(64),
      "sha256:v2:" + "3".repeat(64),
    ]);

    await captureJob("blocked");
    await seed.query(`
      INSERT INTO pricing_stage8_capture_artifacts_v2 (
        job_id, attempt, engine_evidence_digest, engine_captured_at, engine_payload_json,
        combined_evidence_digest, combined_payload_json, combined_passed,
        combined_write_result, completed_at
      )
      SELECT id, 1, $1, now(), '{}'::jsonb, $2, $3::jsonb, false, 'stored', now()
      FROM pricing_stage8_capture_jobs_v2 WHERE target_generation = 41
    `, [
      "sha256:v2:" + "2".repeat(64),
      "sha256:v2:" + "3".repeat(64),
      JSON.stringify({ blockers: [{ code: "engine_inventory_drift" }] }),
    ]);
    await advancePricingReleaseOrchestrationV2(database, readers);
    let control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations[0]).toMatchObject({
      step: "materialize_pair",
      cycle: 2,
      status: "active",
      target_generation: null,
    });

    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'capture', target_generation = 41, recovery_generation = 42
      WHERE id = $1
    `, [staged.orchestrationId]);
    // A transient blocker (busy authority window / backlog) re-captures instead of dying.
    const runId = randomUUID();
    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'capture', target_generation = 41, recovery_generation = 42,
          status = 'active', cycle = 1, stage5_run_id = $2
      WHERE id = $1
    `, [staged.orchestrationId, runId]);
    await seed.query(`
      UPDATE pricing_stage8_capture_artifacts_v2
      SET combined_payload_json = $1::jsonb, created_at = now()
    `, [JSON.stringify({ blockers: [{ code: "authority_changed_during_validation_window" }] })]);
    // The quiet gate: without a long-completed rollout the transient re-capture does not stage.
    await advancePricingReleaseOrchestrationV2(database, readers);
    const quietRow = await readPricingReleaseOrchestrationControlV2(database);
    expect(quietRow.orchestrations[0]).toMatchObject({ step: "capture", status: "active" });
    const quiet = await seed.query<{ count: string }>(`
      SELECT count(*)::text FROM pricing_stage8_capture_jobs_v2 WHERE target_generation = 41
    `, []);
    expect(Number(quiet.rows[0]!.count)).toBe(1);
    await seed.query(`
      INSERT INTO pricing_stage5_runs_v2 (
        run_id, schema_version, plan_digest, commerce_inventory_digest,
        engine_scan_first_digest, engine_scan_second_digest,
        openkeys_scan_first_digest, openkeys_scan_second_digest,
        service_inventory_digest, funding_plan_digest,
        target_generation, target_digest, recovery_generation, recovery_digest,
        inventory_artifact, plan_artifact, blocker_count, status
      ) VALUES ($1, 2, $2, $3, $3, $3, $3, $3, $3, $3, 41, NULL, 42, NULL,
                '{}'::jsonb, '{}'::jsonb, 0, 'materializing')
    `, [runId, "sha256:v2:" + "9".repeat(64), "sha256:v2:" + "8".repeat(64)]);
    for (const [generation, kind, planDigest] of [
      [41, "target", "sha256:v2:" + "a".repeat(64)],
      [42, "recovery", "sha256:v2:" + "b".repeat(64)],
    ] as const) {
      await seed.query(`
        INSERT INTO pricing_release_plans_v2 (
          generation, release_kind, schema_version,
          commerce_inventory_digest, engine_inventory_digest,
          openkeys_inventory_digest, service_inventory_digest,
          policy_manifest_digest, assignment_manifest_digest,
          funding_manifest_digest, engine_release_digest, content_digest, status
        ) VALUES ($1, $2, 2, $3, $3, $3, $3, $3, $3, NULL, NULL, $4, 'materializing')
      `, [generation, kind, "sha256:v2:" + "8".repeat(64), planDigest]);
    }
    await seed.query(`
      INSERT INTO pricing_shadow_rollouts_v2 (
        idempotency_key, stage5_run_id, target_generation, target_digest,
        recovery_generation, recovery_digest, catalog_generation,
        main_catalog_digest, openkeys_catalog_digest, switch_generation, switch_digest,
        engine_inventory_digest, assignment_manifest_digest, policy_manifest_digest,
        rollout_digest, assignment_count, job_count, actor_id, reason, status, completed_at
      ) VALUES ($1, $2, 41, $3, 42, $4, 7, $5, $5,
                7, $5, $5, $5, $5, $5,
                1, 1, 'pricing-control-worker:integration', 'drive the successor pair live',
                'confirmed', now() - interval '15 minutes 20 seconds')
    `, [randomUUID(), runId, "sha256:v2:" + "a".repeat(64), "sha256:v2:" + "b".repeat(64),
        "sha256:v2:" + "c".repeat(64)]);
    // 15 min 20 s after the rollout the 15 min 30 s window still reaches the churn: no staging.
    await advancePricingReleaseOrchestrationV2(database, readers);
    const notYet = await seed.query<{ count: string }>(`
      SELECT count(*)::text FROM pricing_stage8_capture_jobs_v2 WHERE target_generation = 41
    `, []);
    expect(Number(notYet.rows[0]!.count)).toBe(1);
    await seed.query(`
      UPDATE pricing_shadow_rollouts_v2 SET completed_at = now() - interval '20 minutes'
      WHERE stage5_run_id = $1
    `, [runId]);
    await advancePricingReleaseOrchestrationV2(database, readers);
    const transient = await readPricingReleaseOrchestrationControlV2(database);
    expect(transient.orchestrations[0]).toMatchObject({ step: "capture", status: "active" });
    const stagedCaptures = await seed.query<{ count: string }>(`
      SELECT count(*)::text FROM pricing_stage8_capture_jobs_v2 WHERE target_generation = 41
    `, []);
    expect(Number(stagedCaptures.rows[0]!.count)).toBeGreaterThan(1);

    // A non-drift, non-transient blocker still fails closed.
    await seed.query(`
      UPDATE pricing_stage8_capture_jobs_v2
      SET status = 'blocked', completed_at = now(), result_passed = false,
          result_engine_evidence_digest = $1, result_combined_evidence_digest = $2,
          created_at = now()
      WHERE target_generation = 41 AND status IN ('pending', 'retry')
    `, ["sha256:v2:" + "5".repeat(64), "sha256:v2:" + "6".repeat(64)]);
    await captureJob("blocked");
    await seed.query(`
      UPDATE pricing_stage8_capture_artifacts_v2
      SET combined_payload_json = $1::jsonb, created_at = now()
    `, [JSON.stringify({ blockers: [{ code: "openkeys_target_policy_not_one_to_one" }] })]);
    await advancePricingReleaseOrchestrationV2(database, readers);
    control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations[0]).toMatchObject({ step: "capture", status: "dead" });
    expect(control.orchestrations[0]?.last_error).toContain("openkeys_target_policy_not_one_to_one");
  });

  it("re-cycles a funding normalization that died on inventory drift", async () => {
    const staged = await stageIntent();
    const runId = randomUUID();
    const digest = (label: string) => "sha256:v2:" + Buffer.from(label.padEnd(64, "0")).toString("hex").slice(0, 64);
    await seed.query(`
      INSERT INTO pricing_stage5_runs_v2 (
        run_id, schema_version, plan_digest, commerce_inventory_digest,
        engine_scan_first_digest, engine_scan_second_digest,
        openkeys_scan_first_digest, openkeys_scan_second_digest,
        service_inventory_digest, funding_plan_digest,
        target_generation, target_digest, recovery_generation, recovery_digest,
        inventory_artifact, plan_artifact, blocker_count, status
      ) VALUES ($1, 2, $2, $3, $3, $3, $4, $4, $5, $6, 41, NULL, 42, NULL,
                '{}'::jsonb, '{}'::jsonb, 0, 'materializing')
    `, [runId, digest("plan"), digest("commerce"), digest("openkeys"), digest("service"), digest("funding")]);
    for (const [generation, kind, planDigest] of [
      [41, "target", digest("target-plan")],
      [42, "recovery", digest("recovery-plan")],
    ] as const) {
      await seed.query(`
        INSERT INTO pricing_release_plans_v2 (
          generation, release_kind, schema_version,
          commerce_inventory_digest, engine_inventory_digest,
          openkeys_inventory_digest, service_inventory_digest,
          policy_manifest_digest, assignment_manifest_digest,
          funding_manifest_digest, engine_release_digest, content_digest, status
        ) VALUES ($1, $2, 2, $3, $3, $4, $5, $6, $6, NULL, NULL, $7, 'materializing')
      `, [generation, kind, digest("inventory"), digest("openkeys"), digest("service"),
        digest("policy-manifest"), planDigest]);
    }
    await seed.query(`
      INSERT INTO pricing_release_control_jobs_v2 (
        job_kind, release_generation, release_digest,
        idempotency_key, payload_digest, status, attempts, last_error
      ) VALUES ('normalize_funding', 41, $1, $2, $3, 'dead', 1,
                'engine identity inventory no longer matches the target release plan')
    `, [digest("target-plan"), `pricing:v2:normalize-funding:${digest("plan")}`, digest("payload")]);
    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'normalize_funding', stage5_run_id = $2,
          target_generation = 41, recovery_generation = 42
      WHERE id = $1
    `, [staged.orchestrationId, runId]);

    await advancePricingReleaseOrchestrationV2(database, readers);
    const control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations[0]).toMatchObject({
      step: "materialize_pair",
      cycle: 2,
      status: "active",
      stage5_run_id: null,
    });
    expect(control.orchestrations[0]?.last_error).toContain("fresh cycle");
  });

  it("keeps the rollout step in place on a transient engine outage", async () => {
    const staged = await stageIntent();
    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'rollout', stage5_run_id = $2, target_generation = 41, recovery_generation = 42
      WHERE id = $1
    `, [staged.orchestrationId, randomUUID()]);
    const outage: PricingReleaseOrchestrationReadersV2 = {
      engine: {
        getPricingReleaseHeadV2: async () => null,
        getPricingReleaseInventoryV2: async () => {
          throw new EngineClientError("engine request timed out", undefined, true);
        },
      } as unknown as PricingReleaseOrchestrationReadersV2["engine"],
      openkeys: readers.openkeys,
    };
    await expect(advancePricingReleaseOrchestrationV2(database, outage))
      .rejects.toThrow("timed out");
    const control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations[0]).toMatchObject({ step: "rollout", status: "active" });
  });

  it("confirms only when the engine head attests the orchestrated target", async () => {
    const staged = await stageIntent();
    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'verify', target_generation = 41, recovery_generation = 42,
          evidence_digest = $2, activation_kind = 'successor'
      WHERE id = $1
    `, [staged.orchestrationId, "sha256:v2:" + "4".repeat(64)]);
    const wrongHead: PricingReleaseOrchestrationReadersV2 = {
      engine: engineWithHead({ active_generation: 13, active_digest: "sha256:v2:" + "5".repeat(64) }),
      openkeys: readers.openkeys,
    };
    await advancePricingReleaseOrchestrationV2(database, wrongHead);
    let control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations[0]).toMatchObject({ status: "dead", step: "verify" });
    expect(control.orchestrations[0]?.last_error).toContain("13");

    const second = await stageIntent();
    await seed.query(`
      UPDATE pricing_release_orchestrations_v2
      SET step = 'verify', target_generation = 41, recovery_generation = 42,
          evidence_digest = $2, activation_kind = 'successor'
      WHERE id = $1
    `, [second.orchestrationId, "sha256:v2:" + "4".repeat(64)]);
    const rightHead: PricingReleaseOrchestrationReadersV2 = {
      engine: engineWithHead({ active_generation: 41, active_digest: "sha256:v2:" + "6".repeat(64) }),
      openkeys: readers.openkeys,
    };
    await advancePricingReleaseOrchestrationV2(database, rightHead);
    control = await readPricingReleaseOrchestrationControlV2(database);
    expect(control.orchestrations[0]).toMatchObject({ status: "confirmed", step: "verify" });
    expect(control.orchestrations[0]?.confirmed_at).not.toBeNull();
  });
});
