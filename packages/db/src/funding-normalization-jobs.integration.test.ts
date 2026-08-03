import { createHash, randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type {
  FundingNormalizationPlanV2,
  PricingReleaseInventoryAccountV2,
  PricingReleaseRecoveryLinkV2,
  PricingReleaseV2,
} from "@claude-api/contracts";
import {
  buildFundingNormalizationCoverageV2,
  claimNextFundingNormalizationAccountV2,
  claimNextFundingNormalizationJobV2,
  confirmFundingNormalizationJobV2,
  createDatabase,
  fundingNormalizationEngineInventoryDigestV2,
  fundingNormalizationServiceInventoryDigestV2,
  getFundingNormalizationStageStatusV2,
  getFundingNormalizationStateV2,
  recoverStaleFundingNormalizationJobsV2,
  retryFundingNormalizationJobV2,
  stageFundingNormalizationJobV2,
  storeFundingNormalizationPlanV2,
  type Database,
  type FundingNormalizationServiceInventoryRowV2,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import {
  buildStage5V2Capability,
  buildStage5V2CatalogsAndSwitches,
} from "./pricing-stage5-materializer-v2.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function digest(value: string): string {
  return `sha256:v2:${createHash("sha256").update(value, "utf8").digest("hex")}`;
}

function inventoryAccount(accountId: string): PricingReleaseInventoryAccountV2 {
  return {
    account_id: accountId,
    status: "active",
    multiplier_bp: 10_000,
    balance_nano: "1000",
    reserved_nano: "0",
    spent_nano: "0",
    funding_generation: null,
    funding_head_version: null,
  };
}

function normalizationPlan(
  accountId: string,
  status: "ready" | "normalized",
  normalizationDigest: string,
): FundingNormalizationPlanV2 {
  return {
    account_id: accountId,
    account_status: "active",
    status,
    source: status === "normalized" ? "stored_generation" : "ledger_replay",
    source_state_digest: digest(`source:${status}:${accountId}`),
    normalization_digest: normalizationDigest,
    funding_generation: 7,
    funding_head_version: 1,
    balance_nano: "1000",
    reserved_nano: "0",
    spent_nano: "0",
    lots: [],
    blockers: [],
  };
}

function releaseEngine(): {
  engine: Parameters<typeof confirmFundingNormalizationJobV2>[1];
  releases: Map<number, PricingReleaseV2>;
  links: Map<string, PricingReleaseRecoveryLinkV2>;
} {
  const releases = new Map<number, PricingReleaseV2>();
  const links = new Map<string, PricingReleaseRecoveryLinkV2>();
  return {
    releases,
    links,
    engine: {
      preparePricingReleaseV2: async (release) => {
        const result = releases.has(release.generation) ? "unchanged" as const : "stored" as const;
        releases.set(release.generation, structuredClone(release));
        return {
          result,
          identity: {
            generation: release.generation,
            content_digest: release.content_digest,
            release_kind: release.release_kind,
          },
        } as never;
      },
      getPricingReleaseV2: async (generation) => releases.get(generation) ?? null,
      preparePricingReleaseRecoveryLinkV2: async (link) => {
        const key = `${link.target_generation}:${link.recovery_generation}`;
        const result = links.has(key) ? "unchanged" as const : "stored" as const;
        links.set(key, structuredClone(link));
        return {
          result,
          identity: {
            target_generation: link.target_generation,
            recovery_generation: link.recovery_generation,
            link_digest: link.link_digest,
          },
        } as never;
      },
      getPricingReleaseRecoveryLinkV2: async (targetGeneration, recoveryGeneration) =>
        links.get(`${targetGeneration}:${recoveryGeneration}`) ?? null,
    },
  };
}

describe.runIf(Boolean(connectionString))("funding normalization jobs", () => {
  let admin: Client;
  let seed: Client;
  let database: Database;
  let databaseName: string;
  let databaseUrl: string;

  beforeAll(async () => {
    databaseName = `fundnorm_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    databaseUrl = url.toString();
    seed = new Client({ connectionString: databaseUrl });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(databaseUrl, "funding-normalization-test");
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

  async function seedTargetRelease(
    generation: bigint,
    inventory: readonly PricingReleaseInventoryAccountV2[],
    services: readonly FundingNormalizationServiceInventoryRowV2[],
    stage5Status: "planned" | "materializing" = "materializing",
  ): Promise<{ planDigest: string; targetPlanDigest: string; recoveryGeneration: bigint }> {
    for (const service of services) {
      await seed.query(`
        INSERT INTO service_account_inventory_v2 (
          service_id, engine_account_id, purpose, responsible,
          status, source_version, content_digest
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
      `, [
        service.serviceId,
        service.engineAccountId,
        service.purpose,
        service.responsible,
        service.status,
        service.sourceVersion,
        service.contentDigest,
      ]);
    }
    const recoveryGeneration = generation + 1n;
    const targetPlanDigest = digest(`release-plan:${generation}`);
    const recoveryPlanDigest = digest(`release-plan:${recoveryGeneration}`);
    const planDigest = digest(`stage5-plan:${generation}`);
    const fundingPlanDigest = digest(`funding-plan:${generation}`);
    const capability = buildStage5V2Capability();
    const { catalogs, switches } = buildStage5V2CatalogsAndSwitches();
    const balancePolicyDigest = digest(`policy:balance:${generation}`);
    const servicePolicyDigest = digest(`policy:service:${generation}`);
    await seed.query(`
      INSERT INTO pricing_policy_documents_v2 (
        policy_id, policy_version, owner_type, owner_id, account_class,
        product_id, billing_mode, schema_version, capability_generation,
        capability_digest, catalog_generation, catalog_digest,
        switch_generation, switch_digest, content_digest
      ) VALUES
        ('test-b2c', 1, 'global_b2c', 'all-b2c', 'b2c',
         $1, 'balance', 2, $2, $3, $4, $5, $6, $7, $8),
        ('test-service', 1, 'service', 'automation', 'service',
         NULL, 'meter_only', 2, $2, $3, NULL, NULL, NULL, NULL, $9)
    `, [
      catalogs[0].product_id,
      capability.generation,
      capability.content_digest,
      catalogs[0].generation,
      catalogs[0].content_digest,
      switches.generation,
      switches.content_digest,
      balancePolicyDigest,
      servicePolicyDigest,
    ]);
    await seed.query(`
      INSERT INTO pricing_release_plans_v2 (
        generation, release_kind, schema_version,
        commerce_inventory_digest, engine_inventory_digest,
        openkeys_inventory_digest, service_inventory_digest,
        policy_manifest_digest, assignment_manifest_digest,
        funding_manifest_digest, engine_release_digest, content_digest, status
      ) VALUES
        ($1, 'target', 2, $3, $4, $5, $6, $7, $8, NULL, NULL, $9, 'planned'),
        ($2, 'recovery', 2, $3, $4, $5, $6, $7, $10, NULL, NULL, $11, 'planned')
    `, [
      generation,
      recoveryGeneration,
      digest("commerce"),
      fundingNormalizationEngineInventoryDigestV2(inventory),
      digest("openkeys"),
      fundingNormalizationServiceInventoryDigestV2(services),
      digest("policies"),
      digest(`assignments:${generation}`),
      targetPlanDigest,
      digest(`assignments:${recoveryGeneration}`),
      recoveryPlanDigest,
    ]);
    for (const account of inventory) {
      const service = services.find((row) => row.engineAccountId === account.account_id);
      for (const releaseGeneration of [generation, recoveryGeneration]) {
        await seed.query(`
          INSERT INTO pricing_release_assignments_v2 (
            release_generation, engine_account_id, account_class,
            owner_context, owner_id, policy_id, policy_version, policy_digest,
            billing_mode, funding_generation, purpose, responsible, assignment_digest
          ) VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, NULL, $9, $10, $11)
        `, [
          releaseGeneration,
          account.account_id,
          service ? "service" : "b2c",
          service ? "service" : "commerce",
          service?.serviceId ?? `user:${account.account_id}`,
          service ? "test-service" : "test-b2c",
          service ? servicePolicyDigest : balancePolicyDigest,
          service ? "meter_only" : "balance",
          service?.purpose ?? null,
          service?.responsible ?? null,
          digest(`assignment:${releaseGeneration}:${account.account_id}`),
        ]);
      }
    }
    await seed.query(`
      INSERT INTO pricing_stage5_runs_v2 (
        schema_version, plan_digest, commerce_inventory_digest,
        engine_scan_first_digest, engine_scan_second_digest,
        openkeys_scan_first_digest, openkeys_scan_second_digest,
        service_inventory_digest, funding_plan_digest,
        target_generation, target_digest, recovery_generation, recovery_digest,
        inventory_artifact, plan_artifact, blocker_count, status
      ) VALUES (
        2, $1, $2, $3, $3, $4, $4, $5, $6,
        $7, NULL, $8, NULL, '{}'::jsonb, $9::jsonb, 0, $10
      )
    `, [
      planDigest,
      digest("commerce"),
      fundingNormalizationEngineInventoryDigestV2(inventory),
      digest("openkeys"),
      fundingNormalizationServiceInventoryDigestV2(services),
      fundingPlanDigest,
      generation,
      recoveryGeneration,
      JSON.stringify({
        schema_version: 2,
        plan_digest: planDigest,
        funding_plan_digest: fundingPlanDigest,
        target_generation: Number(generation),
        recovery_generation: Number(recoveryGeneration),
        capability,
        catalogs,
        switches,
        target: {
          generation: Number(generation),
          release_kind: "target",
          content_digest: targetPlanDigest,
        },
        recovery: {
          generation: Number(recoveryGeneration),
          release_kind: "recovery",
          content_digest: recoveryPlanDigest,
        },
      }),
      stage5Status,
    ]);
    return { planDigest, targetPlanDigest, recoveryGeneration };
  }

  it("stages idempotently and confirms only exact ready balance coverage", async () => {
    const customer = inventoryAccount("acct_customer");
    const serviceAccount = inventoryAccount("acct_service");
    const inventory = [customer, serviceAccount];
    const services: FundingNormalizationServiceInventoryRowV2[] = [{
      serviceId: "automation",
      engineAccountId: serviceAccount.account_id,
      purpose: "internal automation",
      responsible: "platform",
      status: "active",
      sourceVersion: 1n,
      contentDigest: digest("service-row"),
    }];
    const targetDigest = digest("customer-funding");
    const lineage = await seedTargetRelease(1n, inventory, services);
    const prepared = releaseEngine();

    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      stage5_status: "materializing",
      target_status: "planned",
      recovery_status: "planned",
      job_id: null,
      job_status: null,
      job_attempts: null,
      job_last_error: null,
      pending_accounts: 0,
      processing_accounts: 0,
      retry_accounts: 0,
      ready_accounts: 0,
      blocker_accounts: 0,
    });

    const firstId = await stageFundingNormalizationJobV2(database, {
      planDigest: lineage.planDigest,
      audit: {
        actorId: "operator@example.test",
        reason: "normalize the reviewed complete inventory",
      },
    });
    const replayId = await stageFundingNormalizationJobV2(database, {
      planDigest: lineage.planDigest,
    });
    expect(replayId).toBe(firstId);
    const audits = await seed.query<{
      actor_id: string;
      target_id: string;
      metadata: Record<string, unknown>;
    }>(`
      SELECT actor_id, target_id, metadata
      FROM audit_log
      WHERE action = 'pricing_stage6_funding_normalization_stage_requested'
      ORDER BY id
    `);
    expect(audits.rows).toEqual([{
      actor_id: "operator@example.test",
      target_id: firstId,
      metadata: expect.objectContaining({
        stage5_plan_digest: lineage.planDigest,
        idempotent_replay: false,
        reason: "normalize the reviewed complete inventory",
      }),
    }]);
    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      stage5_status: "materializing",
      target_status: "materializing",
      recovery_status: "materializing",
      job_id: firstId,
      job_status: "pending",
      job_attempts: 0,
      job_last_error: null,
    });

    const job = await claimNextFundingNormalizationJobV2(database, "worker-a", 300_000);
    expect(job?.id).toBe(firstId);
    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      job_id: firstId,
      job_status: "processing",
      job_attempts: 1,
    });
    const initialState = await getFundingNormalizationStateV2(database, job!);
    expect(buildFundingNormalizationCoverageV2(inventory, initialState)).toMatchObject({
      balanceAccountIds: [customer.account_id],
      serviceAccountIds: [serviceAccount.account_id],
      missingAccountIds: [customer.account_id],
    });
    await expect(confirmFundingNormalizationJobV2(database, prepared.engine, job!, "worker-a", {
      engineInventory: inventory,
    })).rejects.toThrow(/missing or extra balance accounts/);

    await expect(storeFundingNormalizationPlanV2(
      database,
      job!,
      "worker-a",
      normalizationPlan(customer.account_id, "ready", targetDigest),
      "observed",
      1_000,
    )).resolves.toBe("pending");
    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      pending_accounts: 1,
      ready_accounts: 0,
    });
    await expect(claimNextFundingNormalizationAccountV2(database, job!, "worker-a"))
      .resolves.toMatchObject({ engineAccountId: customer.account_id, attempts: 1 });
    await expect(storeFundingNormalizationPlanV2(
      database,
      job!,
      "worker-a",
      normalizationPlan(customer.account_id, "normalized", targetDigest),
      "unchanged",
      1_000,
    )).resolves.toBe("ready");
    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      pending_accounts: 0,
      ready_accounts: 1,
    });

    const readyState = await getFundingNormalizationStateV2(database, job!);
    const readyCoverage = buildFundingNormalizationCoverageV2(inventory, readyState);
    expect(readyCoverage).toMatchObject({
      missingAccountIds: [],
      extraAccountIds: [],
      readyCount: 1,
      blockerCount: 0,
    });
    const resultDigest = await confirmFundingNormalizationJobV2(
      database,
      prepared.engine,
      job!,
      "worker-a",
      {
        engineInventory: inventory,
      },
    );
    expect(resultDigest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
    const stored = await seed.query(`
      SELECT status, result_digest, confirmed_at IS NOT NULL AS confirmed
      FROM pricing_release_control_jobs_v2 WHERE id = $1
    `, [firstId]);
    expect(stored.rows[0]).toEqual({
      status: "confirmed",
      result_digest: resultDigest,
      confirmed: true,
    });
    expect(prepared.releases.size).toBe(2);
    expect(prepared.links.size).toBe(1);
    const finalized = await seed.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_release_plans_v2 WHERE status = 'prepared') AS prepared_plans,
        (SELECT count(*)::int FROM pricing_release_assignments_v2 WHERE funding_generation = 7) AS funded_assignments,
        (SELECT count(*)::int FROM pricing_funding_normalizations_v2 WHERE status = 'ready') AS ready_evidence,
        (SELECT count(*)::int FROM pricing_stage5_prepare_acks_v2
          WHERE artifact_kind IN ('target_release', 'recovery_release', 'recovery_link')) AS final_acks,
        (SELECT count(*)::int FROM pricing_release_control_jobs_v2
          WHERE job_kind IN ('activate_release', 'activate_recovery')) AS activation_jobs,
        (SELECT count(*)::int FROM pricing_release_activation_receipts_v2) AS activation_receipts,
        (SELECT count(*)::int FROM pricing_stage8_evidence_v2) AS stage8_evidence
    `);
    expect(finalized.rows[0]).toEqual({
      prepared_plans: 2,
      funded_assignments: 2,
      ready_evidence: 2,
      final_acks: 3,
      activation_jobs: 0,
      activation_receipts: 0,
      stage8_evidence: 0,
    });
    const assignments = await seed.query<{
      release_generation: string;
      engine_account_id: string;
      account_class: string;
      owner_context: string;
      owner_id: string;
      policy_id: string;
      policy_version: string;
      policy_digest: string;
      billing_mode: string;
      funding_generation: string | null;
      purpose: string | null;
      responsible: string | null;
    }>(`
      SELECT release_generation::text, engine_account_id, account_class, owner_context, owner_id, policy_id,
             policy_version::text, policy_digest, billing_mode,
             funding_generation::text, purpose, responsible
      FROM pricing_release_assignments_v2
      ORDER BY release_generation, engine_account_id COLLATE "C"
    `);
    const assignmentEvidence = (generation: string) => assignments.rows
      .filter((row) => row.release_generation === generation)
      .map(({ release_generation: _releaseGeneration, ...row }) => row);
    expect(assignmentEvidence("1")).toEqual(assignmentEvidence("2"));

    const fundingEvidence = await seed.query<{
      release_generation: string;
      engine_account_id: string;
      funding_generation: string;
      expected_source_digest: string;
      target_funding_digest: string;
      applied_funding_digest: string;
      normalization_source: string;
      blockers: unknown;
      status: string;
    }>(`
      SELECT release_generation::text, engine_account_id, funding_generation::text,
             expected_source_digest, target_funding_digest, applied_funding_digest,
             normalization_source, blockers, status
      FROM pricing_funding_normalizations_v2
      ORDER BY release_generation, engine_account_id COLLATE "C"
    `);
    const normalizationEvidence = (generation: string) => fundingEvidence.rows
      .filter((row) => row.release_generation === generation)
      .map(({ release_generation: _releaseGeneration, ...row }) => row);
    expect(normalizationEvidence("1")).toEqual(normalizationEvidence("2"));

    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      stage5_status: "prepared",
      target_status: "prepared",
      recovery_status: "prepared",
      target_release_digest: prepared.releases.get(1)?.content_digest,
      recovery_release_digest: prepared.releases.get(2)?.content_digest,
      job_id: firstId,
      job_status: "confirmed",
      job_attempts: 1,
      job_last_error: null,
      job_result_digest: resultDigest,
      ready_accounts: 1,
      target_funding_manifest_digest: expect.stringMatching(/^sha256:v2:[0-9a-f]{64}$/),
      recovery_funding_manifest_digest: expect.stringMatching(/^sha256:v2:[0-9a-f]{64}$/),
    });
    await expect(stageFundingNormalizationJobV2(database, { planDigest: lineage.planDigest }))
      .resolves.toBe(firstId);
  }, TEST_TIMEOUT_MS);

  it("refuses staging before Stage 5 has completed its dormant materialization", async () => {
    const account = inventoryAccount("acct_stage5_planned");
    const lineage = await seedTargetRelease(10n, [account], [], "planned");

    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      stage5_status: "planned",
      target_status: "planned",
      recovery_status: "planned",
      job_id: null,
      job_status: null,
      job_attempts: null,
      job_last_error: null,
    });
    await expect(stageFundingNormalizationJobV2(database, { planDigest: lineage.planDigest }))
      .rejects.toThrow(/fully ACKed, unfinalized Stage 5 materialization/);
    const writes = await seed.query<{ jobs: number; materializing_plans: number }>(`
      SELECT
        (SELECT count(*)::int FROM pricing_release_control_jobs_v2) AS jobs,
        (SELECT count(*)::int FROM pricing_release_plans_v2 WHERE status = 'materializing') AS materializing_plans
    `);
    expect(writes.rows[0]).toEqual({ jobs: 0, materializing_plans: 0 });
  }, TEST_TIMEOUT_MS);

  it("derives funding identity only from ready rows and fails closed on remote readback drift", async () => {
    const customer = inventoryAccount("acct_manifest_mismatch");
    const lineage = await seedTargetRelease(4n, [customer], []);
    await stageFundingNormalizationJobV2(database, { planDigest: lineage.planDigest });
    const job = await claimNextFundingNormalizationJobV2(database, "worker-manifest", 300_000);
    await storeFundingNormalizationPlanV2(
      database,
      job!,
      "worker-manifest",
      normalizationPlan(customer.account_id, "normalized", digest("actual-funding")),
      "observed",
      1_000,
    );
    const prepared = releaseEngine();
    prepared.engine.getPricingReleaseV2 = async () => null;
    await expect(confirmFundingNormalizationJobV2(database, prepared.engine, job!, "worker-manifest", {
      engineInventory: [customer],
    })).rejects.toThrow(/readback differs from prepare/);
    const parent = await seed.query(`
      SELECT status, result_digest, confirmed_at FROM pricing_release_control_jobs_v2 WHERE id = $1
    `, [job!.id]);
    expect(parent.rows[0]).toEqual({ status: "processing", result_digest: null, confirmed_at: null });
    const local = await seed.query(`
      SELECT funding_manifest_digest IS NOT NULL AS funding_finalized,
             engine_release_digest, status
      FROM pricing_release_plans_v2 WHERE generation = 4
    `);
    expect(local.rows[0]).toEqual({
      funding_finalized: true,
      engine_release_digest: null,
      status: "materializing",
    });
  }, TEST_TIMEOUT_MS);

  it("recovers expired parent and account leases without losing exact plans", async () => {
    const customer = inventoryAccount("acct_recover");
    const inventory = [customer];
    const lineage = await seedTargetRelease(2n, inventory, []);
    await stageFundingNormalizationJobV2(database, { planDigest: lineage.planDigest });
    const job = await claimNextFundingNormalizationJobV2(database, "worker-old", 30_000);
    const plan = normalizationPlan(customer.account_id, "ready", digest("recover-target"));
    await storeFundingNormalizationPlanV2(database, job!, "worker-old", plan, "observed", 1_000);
    await claimNextFundingNormalizationAccountV2(database, job!, "worker-old");
    await seed.query(`
      UPDATE pricing_release_control_jobs_v2 SET locked_at = now() - interval '10 minutes'
      WHERE id = $1
    `, [job!.id]);
    await seed.query(`
      UPDATE pricing_funding_normalizations_v2 SET locked_at = now() - interval '10 minutes'
      WHERE release_generation = 2 AND engine_account_id = $1
    `, [customer.account_id]);

    await expect(recoverStaleFundingNormalizationJobsV2(database, 30_000))
      .resolves.toEqual({ parents: 1, accounts: 1 });
    const recovered = await seed.query(`
      SELECT parent.status AS parent_status, parent.locked_by AS parent_worker,
             account.status AS account_status, account.locked_by AS account_worker,
             account.expected_source_digest
      FROM pricing_release_control_jobs_v2 parent
      JOIN pricing_funding_normalizations_v2 account
        ON account.release_generation = parent.release_generation
      WHERE parent.id = $1
    `, [job!.id]);
    expect(recovered.rows[0]).toEqual({
      parent_status: "retry",
      parent_worker: null,
      account_status: "retry",
      account_worker: null,
      expected_source_digest: plan.source_state_digest,
    });
  }, TEST_TIMEOUT_MS);

  it("rejects queue writes after the parent lease has been released", async () => {
    const customer = inventoryAccount("acct_parent_lease");
    const lineage = await seedTargetRelease(12n, [customer], []);
    await stageFundingNormalizationJobV2(database, { planDigest: lineage.planDigest });
    const job = await claimNextFundingNormalizationJobV2(database, "worker-lease", 300_000);
    await retryFundingNormalizationJobV2(
      database,
      job!,
      "worker-lease",
      "test parent release",
      1_000,
    );
    await expect(getFundingNormalizationStageStatusV2(database, lineage.planDigest)).resolves.toMatchObject({
      job_status: "retry",
      job_attempts: 1,
      job_last_error: "test parent release",
    });

    await expect(storeFundingNormalizationPlanV2(
      database,
      job!,
      "worker-lease",
      normalizationPlan(customer.account_id, "ready", digest("late-plan")),
      "observed",
      1_000,
    )).rejects.toThrow(/lost its lease/);
    const rows = await seed.query<{ count: number }>(`
      SELECT count(*)::int AS count FROM pricing_funding_normalizations_v2
    `);
    expect(rows.rows[0]).toEqual({ count: 0 });
  }, TEST_TIMEOUT_MS);

  it("keeps legacy in-flight local retryable and persists other blockers fail closed", async () => {
    const customer = inventoryAccount("acct_blocked");
    const lineage = await seedTargetRelease(3n, [customer], []);
    await stageFundingNormalizationJobV2(database, { planDigest: lineage.planDigest });
    const job = await claimNextFundingNormalizationJobV2(database, "worker-b", 300_000);
    const blocked: FundingNormalizationPlanV2 = {
      account_id: customer.account_id,
      account_status: "active",
      status: "blocked",
      source: "ledger_replay",
      source_state_digest: digest("blocked-source"),
      normalization_digest: null,
      funding_generation: null,
      funding_head_version: null,
      balance_nano: "1000",
      reserved_nano: "0",
      spent_nano: "0",
      lots: [],
      blockers: [{ code: "active_legacy_reservation", detail: "request is still settling" }],
    };
    await expect(storeFundingNormalizationPlanV2(
      database, job!, "worker-b", blocked, "blocked", 1_000,
    )).resolves.toBe("retry");

    const technical: FundingNormalizationPlanV2 = {
      ...blocked,
      source_state_digest: digest("technical-source"),
      blockers: [{ code: "aggregate_reservation_mismatch", detail: "aggregate differs" }],
    };
    await expect(storeFundingNormalizationPlanV2(
      database, job!, "worker-b", technical, "blocked", 1_000,
    )).resolves.toBe("blocker");
    const stored = await seed.query(`
      SELECT status, funding_generation::text, target_funding_digest,
             expected_source_digest, normalization_source, blockers
      FROM pricing_funding_normalizations_v2
      WHERE release_generation = 3 AND engine_account_id = $1
    `, [customer.account_id]);
    expect(stored.rows[0]).toEqual({
      status: "blocker",
      funding_generation: null,
      target_funding_digest: null,
      expected_source_digest: technical.source_state_digest,
      normalization_source: technical.source,
      blockers: technical.blockers,
    });
  }, TEST_TIMEOUT_MS);
});
