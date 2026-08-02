import { randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  buildStage5ServiceInventoryV2,
  collectStage8CombinedEvidenceV2,
  createDatabase,
  stage5V2CommerceInventoryDigest,
  stage5V2Digest,
  stage8EngineEvidenceDigestV2,
  type Stage5V2OpenKeysReader,
  type Stage8EngineEvidenceV2,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const TARGET_GENERATION = 80_101n;
const RECOVERY_GENERATION = 80_102n;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function digest(label: string): string {
  return stage5V2Digest("stage8-integration", label);
}

function engineEvidence(input: {
  engineInventoryDigest: string;
  fundingDigest: string;
  targetEngineDigest: string;
  recoveryEngineDigest: string;
  passed?: boolean;
}): Stage8EngineEvidenceV2 {
  const captured = BigInt(Math.floor(Date.now() / 1_000));
  const passed = input.passed ?? true;
  const report: Stage8EngineEvidenceV2 = {
    schema_version: 2n,
    captured_ts: captured,
    window_start_ts: captured - 100n,
    window_end_ts: captured - 10n,
    min_samples_per_provider: 1n,
    gemini_client_admissions: 1n,
    passed,
    release: {
      target_generation: TARGET_GENERATION,
      target_digest: input.targetEngineDigest,
      recovery_generation: RECOVERY_GENERATION,
      recovery_digest: input.recoveryEngineDigest,
      recovery_link_digest: digest("recovery-link"),
      inventory_digest: input.engineInventoryDigest,
      funding_digest: input.fundingDigest,
      target_assignment_count: 1n,
      recovery_assignment_count: 1n,
      active_head: null,
    },
    runtime_manifest: {
      generation: 3n,
      digest: digest("runtime-manifest"),
      capabilities: [{
        schema_version: 2n,
        generation: 3n,
        digest: digest("capability"),
      }],
    },
    catalogs: [
      {
        product_id: "main",
        generation: 3n,
        schema_version: 2n,
        capability_generation: 3n,
        capability_digest: digest("capability"),
        content_digest: digest("main-catalog"),
        enabled_entries: 3n,
      },
      {
        product_id: "openkeys",
        generation: 3n,
        schema_version: 2n,
        capability_generation: 3n,
        capability_digest: digest("capability"),
        content_digest: digest("openkeys-catalog"),
        enabled_entries: 2n,
      },
    ],
    switches: {
      generation: 3n,
      schema_version: 2n,
      capability_generation: 3n,
      capability_digest: digest("capability"),
      content_digest: digest("switches"),
      entries: 14n,
    },
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
    financial_samples: [],
    engine_inventory_digest: input.engineInventoryDigest,
    funding_digest: input.fundingDigest,
    shadow_digest: digest("shadow"),
    runtime_floor_digest: digest("runtime-floor"),
    legacy_inflight_count: 0n,
    blockers: passed ? [] : [{
      code: "live_runtime_below_release_v2_floor",
      count: 1n,
      subject_digests: [`sha256:v1:${"1".repeat(64)}`],
    }],
    evidence_digest: `sha256:v2:${"0".repeat(64)}`,
  };
  report.evidence_digest = stage8EngineEvidenceDigestV2(report);
  return report;
}

describe.runIf(Boolean(connectionString))("Stage 8 combined commerce evidence", () => {
  let admin: Client;
  let seed: Client;
  let databaseName: string;
  let databaseUrl: string;

  beforeAll(async () => {
    databaseName = `stage8_v2_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    databaseUrl = url.toString();
    seed = new Client({ connectionString: databaseUrl });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
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

  async function seedPreparedPair(options: { recoveryOwnerDrift?: boolean } = {}): Promise<{
    report: Stage8EngineEvidenceV2;
    openkeys: Stage5V2OpenKeysReader;
    accountId: string;
  }> {
    const userId = randomUUID();
    const recordId = randomUUID();
    const accountId = "acct_stage8_combined_b2c";
    await seed.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'Stage 8 combined', true, 'active')
    `, [userId, `${userId}@example.test`]);
    await seed.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start
      ) VALUES ($1, 'b2c', 0, 4000, now())
    `, [userId]);
    await seed.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 4000, 'active')
    `, [recordId, userId, accountId]);

    const commerceDigest = stage5V2CommerceInventoryDigest({
      accounts: [{
        user_id: userId,
        engine_account_record_id: recordId,
        engine_account_id: accountId,
        account_class: "b2c",
        profile_multiplier_bp: 4_000,
        commerce_multiplier_bp: 4_000,
        commerce_status: "active",
      }],
      invitations: [],
    });
    const serviceDigest = buildStage5ServiceInventoryV2([]).inventory_digest;
    const engineInventoryDigest = digest("engine-inventory");
    const openkeysDigest = digest("openkeys-inventory-empty");
    const fundingDigest = digest("funding-manifest");
    const targetPlanDigest = digest("target-plan");
    const recoveryPlanDigest = digest("recovery-plan");
    const targetEngineDigest = digest("target-engine-release");
    const recoveryEngineDigest = digest("recovery-engine-release");
    const policyDigest = digest("policy");
    await seed.query(`
      INSERT INTO pricing_policy_documents_v2 (
        policy_id, policy_version, owner_type, owner_id, account_class,
        product_id, billing_mode, schema_version,
        capability_generation, capability_digest,
        catalog_generation, catalog_digest, switch_generation, switch_digest,
        content_digest
      ) VALUES (
        'release-v2:b2c:global', 1, 'global_b2c', 'global', 'b2c',
        'main', 'balance', 2, 3, $1, 3, $2, 3, $3, $4
      )
    `, [digest("capability"), digest("main-catalog"), digest("switches"), policyDigest]);
    await seed.query(`
      INSERT INTO pricing_release_plans_v2 (
        generation, release_kind, schema_version,
        commerce_inventory_digest, engine_inventory_digest,
        openkeys_inventory_digest, service_inventory_digest,
        policy_manifest_digest, assignment_manifest_digest,
        funding_manifest_digest, engine_release_digest, content_digest, status
      ) VALUES
        ($1, 'target', 2, $3, $4, $5, $6, $7, $8, NULL, NULL, $10, 'materializing'),
        ($2, 'recovery', 2, $3, $4, $5, $6, $7, $9, NULL, NULL, $11, 'materializing')
    `, [
      TARGET_GENERATION,
      RECOVERY_GENERATION,
      commerceDigest,
      engineInventoryDigest,
      openkeysDigest,
      serviceDigest,
      digest("policy-manifest"),
      digest("target-assignment-manifest"),
      digest("recovery-assignment-manifest"),
      targetPlanDigest,
      recoveryPlanDigest,
    ]);
    for (const generation of [TARGET_GENERATION, RECOVERY_GENERATION]) {
      const ownerId = generation === RECOVERY_GENERATION && options.recoveryOwnerDrift
        ? randomUUID()
        : userId;
      await seed.query(`
        INSERT INTO pricing_release_assignments_v2 (
          release_generation, engine_account_id, account_class, owner_context,
          owner_id, policy_id, policy_version, policy_digest, billing_mode,
          funding_generation, purpose, responsible, assignment_digest
        ) VALUES (
          $1, $2, 'b2c', 'commerce', $3,
          'release-v2:b2c:global', 1, $4, 'balance', NULL, NULL, NULL, $5
        )
      `, [generation, accountId, ownerId, policyDigest, digest(`assignment:${generation}`)]);
      await seed.query(`
        INSERT INTO pricing_funding_normalizations_v2 (
          release_generation, engine_account_id, funding_generation,
          expected_source_digest, target_funding_digest, applied_funding_digest,
          normalization_source, blockers, status
        ) VALUES ($1, $2, 7, $3, $4, $4, 'ledger_replay', NULL, 'ready')
      `, [generation, accountId, digest("funding-source"), digest("account-funding")]);
      await seed.query(`
        UPDATE pricing_release_assignments_v2
        SET funding_generation = 7
        WHERE release_generation = $1 AND engine_account_id = $2
      `, [generation, accountId]);
    }
    await seed.query(`
      UPDATE pricing_release_plans_v2 SET
        funding_manifest_digest = $3,
        engine_release_digest = CASE generation WHEN $1 THEN $4 ELSE $5 END,
        status = 'prepared', updated_at = now()
      WHERE generation IN ($1, $2)
    `, [
      TARGET_GENERATION,
      RECOVERY_GENERATION,
      fundingDigest,
      targetEngineDigest,
      recoveryEngineDigest,
    ]);
    return {
      accountId,
      report: engineEvidence({
        engineInventoryDigest,
        fundingDigest,
        targetEngineDigest,
        recoveryEngineDigest,
      }),
      openkeys: {
        getPage: async () => ({
          inventory_digest: openkeysDigest,
          accounts: [],
          next_after_account_id: null,
        }),
      },
    };
  }

  it("stores one passed identity bound to exact commerce, OpenKeys and engine evidence", async () => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, "stage8-combined-pass");
    try {
      const report = await collectStage8CombinedEvidenceV2(database, seeded.openkeys, seeded.report);
      expect(report).toMatchObject({
        schema_version: 2,
        passed: true,
        write_result: "stored",
        blocker_count: "0",
        legacy_inflight_count: "0",
      });
      expect(report.evidence_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
      expect(report.source.engine_evidence_digest).toBe(seeded.report.evidence_digest);
      expect(JSON.stringify(report)).not.toContain(seeded.accountId);
      const stored = await seed.query<{
        evidence_digest: string;
        passed: boolean;
        blocker_count: string;
      }>(`
        SELECT evidence_digest, passed, blocker_count::text
        FROM pricing_stage8_evidence_v2
      `);
      expect(stored.rows).toEqual([{
        evidence_digest: report.evidence_digest,
        passed: true,
        blocker_count: "0",
      }]);
    } finally {
      await database.pool.end();
    }
  });

  it("persists a failed snapshot when commerce inventory changed after release preparation", async () => {
    const seeded = await seedPreparedPair();
    await seed.query("UPDATE engine_accounts SET mult_bp = 4100 WHERE engine_account_id = $1", [seeded.accountId]);
    const database = createDatabase(databaseUrl, "stage8-combined-drift");
    try {
      const report = await collectStage8CombinedEvidenceV2(database, seeded.openkeys, seeded.report);
      expect(report.passed).toBe(false);
      expect(report.write_result).toBe("stored");
      expect(report.blockers.map((blocker) => blocker.code)).toEqual(expect.arrayContaining([
        "target_release_identity_drift",
        "recovery_release_identity_drift",
      ]));
      expect(JSON.stringify(report)).not.toContain(seeded.accountId);
      const stored = await seed.query<{ passed: boolean; blocker_count: string }>(`
        SELECT passed, blocker_count::text FROM pricing_stage8_evidence_v2
      `);
      expect(stored.rows[0]!.passed).toBe(false);
      expect(BigInt(stored.rows[0]!.blocker_count)).toBeGreaterThan(0n);
    } finally {
      await database.pool.end();
    }
  });

  it("rejects target and recovery assignments with different commerce ownership", async () => {
    const seeded = await seedPreparedPair({ recoveryOwnerDrift: true });
    const database = createDatabase(databaseUrl, "stage8-combined-lineage-drift");
    try {
      const report = await collectStage8CombinedEvidenceV2(database, seeded.openkeys, seeded.report);
      expect(report.passed).toBe(false);
      expect(report.blockers).toContainEqual(expect.objectContaining({
        source: "commerce",
        code: "target_recovery_commerce_lineage_mismatch",
      }));
    } finally {
      await database.pool.end();
    }
  });

  it("carries engine runtime blockers into immutable combined evidence", async () => {
    const seeded = await seedPreparedPair();
    const blocked = engineEvidence({
      engineInventoryDigest: seeded.report.engine_inventory_digest,
      fundingDigest: seeded.report.funding_digest,
      targetEngineDigest: seeded.report.release.target_digest!,
      recoveryEngineDigest: seeded.report.release.recovery_digest!,
      passed: false,
    });
    const database = createDatabase(databaseUrl, "stage8-combined-engine-blocker");
    try {
      const report = await collectStage8CombinedEvidenceV2(database, seeded.openkeys, blocked);
      expect(report.passed).toBe(false);
      expect(report.blockers).toContainEqual(expect.objectContaining({
        source: "engine",
        code: "live_runtime_below_release_v2_floor",
      }));
    } finally {
      await database.pool.end();
    }
  });
});
