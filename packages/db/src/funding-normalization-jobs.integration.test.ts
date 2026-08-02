import { createHash, randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { FundingNormalizationPlanV2, PricingReleaseInventoryAccountV2 } from "@claude-api/contracts";
import {
  buildFundingNormalizationCoverageV2,
  claimNextFundingNormalizationAccountV2,
  claimNextFundingNormalizationJobV2,
  confirmFundingNormalizationJobV2,
  createDatabase,
  fundingNormalizationEngineInventoryDigestV2,
  fundingNormalizationManifestDigestV2,
  fundingNormalizationServiceInventoryDigestV2,
  getFundingNormalizationStateV2,
  recoverStaleFundingNormalizationJobsV2,
  stageFundingNormalizationJobV2,
  storeFundingNormalizationPlanV2,
  type Database,
  type FundingNormalizationServiceInventoryRowV2,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

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
    fundingManifestDigest: string,
  ): Promise<string> {
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
    const releaseDigest = digest(`release:${generation}`);
    await seed.query(`
      INSERT INTO pricing_release_plans_v2 (
        generation, release_kind, schema_version,
        commerce_inventory_digest, engine_inventory_digest,
        openkeys_inventory_digest, service_inventory_digest,
        policy_manifest_digest, assignment_manifest_digest,
        funding_manifest_digest, engine_release_digest, content_digest
      ) VALUES ($1, 'target', 2, $2, $3, $4, $5, $6, $7, $8, $9, $10)
    `, [
      generation,
      digest("commerce"),
      fundingNormalizationEngineInventoryDigestV2(inventory),
      digest("openkeys"),
      fundingNormalizationServiceInventoryDigestV2(services),
      digest("policies"),
      digest("assignments"),
      fundingManifestDigest,
      digest("engine-release"),
      releaseDigest,
    ]);
    return releaseDigest;
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
    const fundingManifest = fundingNormalizationManifestDigestV2([{
      engineAccountId: customer.account_id,
      fundingGeneration: 7n,
      appliedFundingDigest: targetDigest,
    }]);
    const releaseDigest = await seedTargetRelease(1n, inventory, services, fundingManifest);

    const firstId = await stageFundingNormalizationJobV2(database, {
      releaseGeneration: 1n,
      releaseDigest,
    });
    const replayId = await stageFundingNormalizationJobV2(database, {
      releaseGeneration: 1n,
      releaseDigest,
    });
    expect(replayId).toBe(firstId);

    const job = await claimNextFundingNormalizationJobV2(database, "worker-a", 300_000);
    expect(job?.id).toBe(firstId);
    const initialState = await getFundingNormalizationStateV2(database, job!);
    expect(buildFundingNormalizationCoverageV2(inventory, initialState)).toMatchObject({
      balanceAccountIds: [customer.account_id],
      serviceAccountIds: [serviceAccount.account_id],
      missingAccountIds: [customer.account_id],
    });
    await expect(confirmFundingNormalizationJobV2(database, job!, "worker-a", {
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

    const readyState = await getFundingNormalizationStateV2(database, job!);
    const readyCoverage = buildFundingNormalizationCoverageV2(inventory, readyState);
    expect(readyCoverage).toMatchObject({
      missingAccountIds: [],
      extraAccountIds: [],
      readyCount: 1,
      blockerCount: 0,
    });
    const resultDigest = await confirmFundingNormalizationJobV2(database, job!, "worker-a", {
      engineInventory: inventory,
    });
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
  }, TEST_TIMEOUT_MS);

  it("refuses confirmation when applied rows differ from the target funding manifest", async () => {
    const customer = inventoryAccount("acct_manifest_mismatch");
    const releaseDigest = await seedTargetRelease(
      4n,
      [customer],
      [],
      digest("different-funding-manifest"),
    );
    await stageFundingNormalizationJobV2(database, { releaseGeneration: 4n, releaseDigest });
    const job = await claimNextFundingNormalizationJobV2(database, "worker-manifest", 300_000);
    await storeFundingNormalizationPlanV2(
      database,
      job!,
      "worker-manifest",
      normalizationPlan(customer.account_id, "normalized", digest("actual-funding")),
      "observed",
      1_000,
    );

    await expect(confirmFundingNormalizationJobV2(database, job!, "worker-manifest", {
      engineInventory: [customer],
    })).rejects.toThrow(/does not match the immutable target release plan/);
    const parent = await seed.query(`
      SELECT status, result_digest, confirmed_at FROM pricing_release_control_jobs_v2 WHERE id = $1
    `, [job!.id]);
    expect(parent.rows[0]).toEqual({ status: "processing", result_digest: null, confirmed_at: null });
  }, TEST_TIMEOUT_MS);

  it("recovers expired parent and account leases without losing exact plans", async () => {
    const customer = inventoryAccount("acct_recover");
    const inventory = [customer];
    const releaseDigest = await seedTargetRelease(
      2n,
      inventory,
      [],
      fundingNormalizationManifestDigestV2([]),
    );
    await stageFundingNormalizationJobV2(database, { releaseGeneration: 2n, releaseDigest });
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

  it("keeps legacy in-flight local retryable and persists other blockers fail closed", async () => {
    const customer = inventoryAccount("acct_blocked");
    const releaseDigest = await seedTargetRelease(
      3n,
      [customer],
      [],
      fundingNormalizationManifestDigestV2([]),
    );
    await stageFundingNormalizationJobV2(database, { releaseGeneration: 3n, releaseDigest });
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
