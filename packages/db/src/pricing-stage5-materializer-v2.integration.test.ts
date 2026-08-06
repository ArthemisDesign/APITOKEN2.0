import { randomUUID } from "node:crypto";
import type {
  OpenKeysPricingInventoryAccountV2,
  PricingCatalogSpec,
  PricingReleaseInventoryAccountV2,
  PricingReleasePolicyV2,
  ProviderSwitchSpec,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import {
  runStage5MaterializerV2,
} from "./pricing-stage5-materializer-v2-store.js";
import {
  buildStage5ServiceInventoryV2,
  stage5V2Digest,
  type Stage5V2OpenKeysReader,
} from "./pricing-stage5-materializer-v2.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function inventoryAccount(
  accountId: string,
  multiplierBp: number,
  status: "active" | "disabled" = "active",
): PricingReleaseInventoryAccountV2 {
  return {
    account_id: accountId,
    status,
    multiplier_bp: multiplierBp,
    balance_nano: "1000000000",
    reserved_nano: "0",
    spent_nano: "0",
    funding_generation: null,
    funding_head_version: null,
  };
}

function openkeysAccount(): OpenKeysPricingInventoryAccountV2 {
  const identity = {
    account_id: "acct_stage5_openkeys",
    source_id: "10000000-0000-4000-8000-000000000001",
    lifecycle: "disabled" as const,
    pricing_contract: "legacy" as const,
    source_multiplier_bp: 3_500,
  };
  return { ...identity, content_digest: stage5V2Digest("integration-openkeys", identity) };
}

function fakeAuthorities(
  inventory: PricingReleaseInventoryAccountV2[],
): {
  engine: EngineClient;
  openkeys: Stage5V2OpenKeysReader;
  prepared: {
    catalogs: Map<string, PricingCatalogSpec>;
    switches: Map<number, ProviderSwitchSpec>;
    policies: Map<string, PricingReleasePolicyV2>;
  };
} {
  const prepared = {
    catalogs: new Map<string, PricingCatalogSpec>(),
    switches: new Map<number, ProviderSwitchSpec>(),
    policies: new Map<string, PricingReleasePolicyV2>(),
  };
  const engine = {
    getPricingReleaseInventoryV2: async () => ({
      accounts: inventory.map((account) => ({ ...account })),
      next_after_account_id: null,
    }),
    getPricingReleaseHeadV2: async () => null,
    getPricingReleaseV2: async () => null,
    getLatestPricingReleasePolicyV2: async (policyId: string) => [...prepared.policies.values()]
      .filter((policy) => policy.policy_id === policyId)
      .sort((left, right) => right.policy_version - left.policy_version)[0] ?? null,
    preparePricingCatalog: async (catalog: PricingCatalogSpec) => {
      const key = `${catalog.product_id}:${catalog.generation}`;
      const result = prepared.catalogs.has(key) ? "unchanged" as const : "stored" as const;
      prepared.catalogs.set(key, {
        ...catalog,
        entries: [...catalog.entries].sort((left, right) =>
          left.provider_id.localeCompare(right.provider_id)
          || left.canonical_model_id.localeCompare(right.canonical_model_id)),
      });
      return { result, identity: { catalog } };
    },
    getPricingCatalogVersion: async (productId: string, generation: number) =>
      prepared.catalogs.get(`${productId}:${generation}`) ?? null,
    prepareProviderSwitches: async (switches: ProviderSwitchSpec) => {
      const result = prepared.switches.has(switches.generation) ? "unchanged" as const : "stored" as const;
      prepared.switches.set(switches.generation, switches);
      return { result, identity: { switches } };
    },
    getProviderSwitchVersion: async (generation: number) => prepared.switches.get(generation) ?? null,
    preparePricingReleasePolicyV2: async (policy: PricingReleasePolicyV2) => {
      const key = `${policy.policy_id}:${policy.policy_version}`;
      const result = prepared.policies.has(key) ? "unchanged" as const : "stored" as const;
      prepared.policies.set(key, policy);
      return {
        result,
        identity: {
          policy_id: policy.policy_id,
          policy_version: policy.policy_version,
          content_digest: policy.content_digest,
        },
      };
    },
    getPricingReleasePolicyV2: async (policyId: string, version: number) =>
      prepared.policies.get(`${policyId}:${version}`) ?? null,
  } as unknown as EngineClient;
  const openkeysInventory = [openkeysAccount()];
  const openkeysDigest = stage5V2Digest("integration-openkeys-manifest", openkeysInventory);
  return {
    engine,
    openkeys: {
      getPage: async () => ({
        inventory_digest: openkeysDigest,
        accounts: openkeysInventory,
        next_after_account_id: null,
      }),
    },
    prepared,
  };
}

describe.runIf(Boolean(connectionString))("pricing Stage 5 v2 materializer", () => {
  let admin: Client;
  let seed: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `stage5_v2_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seed = new Client({ connectionString: url.toString() });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "pricing-stage5-v2-test");
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

  async function seedAuthorities(): Promise<PricingReleaseInventoryAccountV2[]> {
    const b2cUser = randomUUID();
    const b2bUser = randomUUID();
    await seed.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'B2C', true, 'active'), ($3, $4, 'B2B', true, 'active')
    `, [b2cUser, `${b2cUser}@example.test`, b2bUser, `${b2bUser}@example.test`]);
    await seed.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start
      ) VALUES ($1, 'b2c', 0, 4000, now()), ($2, 'b2b', NULL, 7000, now())
    `, [b2cUser, b2bUser]);
    await seed.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, 'acct_stage5_b2c', 4000, 'active'),
             ($3, $4, 'acct_stage5_b2b', 7000, 'active')
    `, [randomUUID(), b2cUser, randomUUID(), b2bUser]);
    const b2bPolicyId = `policy:main:b2b:${b2bUser}`;
    await seed.query(`
      INSERT INTO provider_capability_versions (
        generation, schema_version, content_digest, source_runtime, source_revision, observed_at
      ) VALUES (1, 1, 'stage5-capability-v1', 'pricing-stage5-v2-test', 'test-revision', now())
    `);
    await seed.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation,
        capability_digest, content_digest, actor_type, actor_id, reason
      ) VALUES (
        'main', 1, 1, 1, 'stage5-capability-v1', 'stage5-catalog-main-v1',
        'system', 'pricing-stage5-v2-test', 'integration fixture'
      )
    `);
    await seed.query(`
      INSERT INTO pricing_policies (id, owner_type, owner_id, product_id)
      VALUES ($1, 'b2b_client', $2, 'main')
    `, [b2bPolicyId, b2bUser]);
    await seed.query(`
      INSERT INTO pricing_policy_versions (
        policy_id, version, schema_version, product_id, catalog_generation,
        content_digest, actor_type, actor_id, reason
      ) VALUES ($1, 1, 1, 'main', 1, 'b2b-head-v1', 'admin', 'integration-test', 'seed b2b head')
    `, [b2bPolicyId]);
    await seed.query(`
      INSERT INTO pricing_policy_heads (policy_id, current_version, current_digest)
      VALUES ($1, 1, 'b2b-head-v1')
    `, [b2bPolicyId]);
    await seed.query(`
      INSERT INTO pricing_policy_rules (
        policy_id, policy_version, product_id, catalog_generation, rule_id,
        rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
        rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
        retention_eligible, commission_eligible
      ) VALUES
        ($1, 1, 'main', 1, 'provider:anthropic:discount', 'rule-anthropic',
          'provider', 'anthropic', NULL, 'discount', 'managed', 3000, 7000, false, false, false),
        ($1, 1, 'main', 1, 'provider:google:discount', 'rule-google',
          'provider', 'google', NULL, 'discount', 'managed', 3000, 7000, false, false, false)
    `, [b2bPolicyId]);
    const inviteId = randomUUID();
    const expiredInviteId = randomUUID();
    await seed.query(`
      INSERT INTO business_invites (
        id, token_hash, multiplier_bp, expires_at, created_by_actor
      ) VALUES ($1, $2, 6500, now() + interval '30 days', 'integration-test'),
               ($3, $4, 6000, now() - interval '1 hour', 'integration-test')
    `, [inviteId, `hash-${inviteId}`, expiredInviteId, `hash-${expiredInviteId}`]);
    const serviceIdentity = {
      service_id: "internal-worker",
      engine_account_id: "acct_stage5_service",
      purpose: "internal metered automation",
      responsible: "platform-team",
      status: "active" as const,
      source_version: 1,
    };
    const service = buildStage5ServiceInventoryV2([{
      ...serviceIdentity,
      content_digest: stage5V2Digest("integration-service", serviceIdentity),
    }]);
    await seed.query(`
      INSERT INTO service_account_inventory_v2 (
        service_id, engine_account_id, purpose, responsible,
        status, source_version, content_digest
      ) VALUES ($1, $2, $3, $4, $5, $6, $7)
    `, [
      service.accounts[0]!.service_id,
      service.accounts[0]!.engine_account_id,
      service.accounts[0]!.purpose,
      service.accounts[0]!.responsible,
      service.accounts[0]!.status,
      service.accounts[0]!.source_version,
      service.accounts[0]!.content_digest,
    ]);
    return [
      inventoryAccount("acct_stage5_b2b", 7_000),
      inventoryAccount("acct_stage5_b2c", 4_000),
      inventoryAccount("acct_stage5_openkeys", 3_500, "disabled"),
      inventoryAccount("acct_stage5_service", 10_000),
    ];
  }

  it("materializes the full immutable skeleton, proves every engine prepare, and replays exactly", async () => {
    const inventory = await seedAuthorities();
    const authorities = fakeAuthorities(inventory);
    const dryRun = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "dry_run",
    });
    expect(dryRun.plan.blockers).toEqual([]);
    expect(dryRun.writes_committed).toBe(false);

    const applied = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "apply",
      expectedPlanDigest: dryRun.plan.plan_digest,
      audit: {
        actorId: "operator@example.test",
        reason: "materialize the reviewed complete inventory",
      },
    });
    expect(applied).toMatchObject({
      status: "materializing",
      writes_committed: true,
      engine_prepared: true,
      plan: {
        target_digest: null,
        recovery_digest: null,
      },
    });
    expect(applied.run_id).toMatch(/^[0-9a-f-]{36}$/);

    const evidence = await seed.query<{
      status: string;
      target_digest: string | null;
      recovery_digest: string | null;
      blocker_count: string;
    }>(`
      SELECT status, target_digest, recovery_digest, blocker_count::text
      FROM pricing_stage5_runs_v2 WHERE run_id = $1
    `, [applied.run_id]);
    expect(evidence.rows).toEqual([{
      status: "materializing",
      target_digest: null,
      recovery_digest: null,
      blocker_count: "0",
    }]);
    const plans = await seed.query<{
      release_kind: string;
      funding_manifest_digest: string | null;
      engine_release_digest: string | null;
      status: string;
    }>(`
      SELECT release_kind, funding_manifest_digest, engine_release_digest, status
      FROM pricing_release_plans_v2 ORDER BY generation
    `);
    expect(plans.rows).toEqual([
      { release_kind: "target", funding_manifest_digest: null, engine_release_digest: null, status: "planned" },
      { release_kind: "recovery", funding_manifest_digest: null, engine_release_digest: null, status: "planned" },
    ]);
    const counts = await seed.query<{
      assignments: string;
      policies: string;
      acks: string;
      control_jobs: string;
      capability_head: string;
      catalog_heads: string;
      switch_head: string;
      audits: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM pricing_release_assignments_v2) AS assignments,
        (SELECT count(*)::text FROM pricing_policy_documents_v2) AS policies,
        (SELECT count(*)::text FROM pricing_stage5_prepare_acks_v2) AS acks,
        (SELECT count(*)::text FROM pricing_release_control_jobs_v2) AS control_jobs,
        (SELECT count(*)::text FROM provider_capability_head) AS capability_head,
        (SELECT count(*)::text FROM product_catalog_heads) AS catalog_heads,
        (SELECT count(*)::text FROM provider_switch_head) AS switch_head,
        (SELECT count(*)::text FROM audit_log
          WHERE action = 'pricing_stage5_materialization_requested'
            AND actor_id = 'operator@example.test'
            AND metadata->>'plan_digest' = $1
            AND metadata->>'reason' = 'materialize the reviewed complete inventory') AS audits
    `, [dryRun.plan.plan_digest]);
    expect(counts.rows[0]).toEqual({
      assignments: "8",
      policies: "5",
      acks: "8",
      control_jobs: "0",
      capability_head: "0",
      catalog_heads: "0",
      switch_head: "0",
      audits: "1",
    });
    expect(authorities.prepared.catalogs).toHaveLength(2);
    expect(authorities.prepared.switches).toHaveLength(1);
    expect(authorities.prepared.policies).toHaveLength(5);
    const b2bPrepared = [...authorities.prepared.policies.values()]
      .find((policy) => policy.policy_id === "release-v2:b2b:acct_stage5_b2b")!;
    expect(b2bPrepared.rules).toEqual(expect.arrayContaining([
      expect.objectContaining({
        scope: { scope: "provider", provider_id: "anthropic" },
        payable_multiplier_bp: 7_000,
      }),
      expect.objectContaining({
        scope: { scope: "provider", provider_id: "google" },
        payable_multiplier_bp: 7_000,
      }),
    ]));
    expect(b2bPrepared.rules).toHaveLength(2);

    inventory[0]!.balance_nano = "1000000001";
    inventory[0]!.spent_nano = "1";
    const replay = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "apply",
      expectedPlanDigest: dryRun.plan.plan_digest,
    });
    expect(replay.run_id).toBe(applied.run_id);
    expect(replay.plan.plan_digest).toBe(dryRun.plan.plan_digest);
    expect((await seed.query("SELECT 1 FROM pricing_stage5_runs_v2")).rowCount).toBe(1);
  });

  it("reconciles a remote-only policy head before allocating or preparing a version", async () => {
    const inventory = await seedAuthorities();
    const authorities = fakeAuthorities(inventory);
    const baseline = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "dry_run",
    });
    const baselinePolicy = baseline.plan.policies
      .find((policy) => policy.policy_id === "release-v2:b2b:acct_stage5_b2b")!;
    const { content_digest: _baselineDigest, ...baselineIdentity } = baselinePolicy;
    const remotePolicyIdentity = {
      ...baselineIdentity,
      policy_version: baselinePolicy.policy_version + 1,
    };
    const remotePolicy = {
      ...remotePolicyIdentity,
      content_digest: stage5V2Digest("policy", remotePolicyIdentity),
    };
    authorities.prepared.policies.set(
      `${remotePolicy.policy_id}:${remotePolicy.policy_version}`,
      remotePolicy,
    );

    const reconciled = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "dry_run",
    });
    expect(reconciled.plan.blockers).toEqual([]);
    expect(reconciled.plan.policies
      .find((policy) => policy.policy_id === remotePolicy.policy_id))
      .toEqual(remotePolicy);

    const applied = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "apply",
      expectedPlanDigest: reconciled.plan.plan_digest,
    });
    expect(applied.engine_prepared).toBe(true);
    expect(applied.plan.policies
      .find((policy) => policy.policy_id === remotePolicy.policy_id))
      .toEqual(remotePolicy);
  });

  it("persists typed ownership blockers without partially preparing policies or releases", async () => {
    const inventory = await seedAuthorities();
    await seed.query(`
      UPDATE service_account_inventory_v2
      SET engine_account_id = 'acct_stage5_b2c', updated_at = now()
      WHERE service_id = 'internal-worker'
    `);
    const authorities = fakeAuthorities(inventory);
    const dryRun = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "dry_run",
    });
    expect(dryRun.plan.blockers.map((item) => item.blocker_code)).toEqual(expect.arrayContaining([
      "engine_account_owner_collision",
      "engine_account_missing_owner",
    ]));

    const blocked = await runStage5MaterializerV2(database, authorities.engine, authorities.openkeys, {
      mode: "apply",
      expectedPlanDigest: dryRun.plan.plan_digest,
    });
    expect(blocked).toMatchObject({
      status: "blocked",
      writes_committed: true,
      engine_prepared: false,
    });
    const counts = await seed.query<{
      runs: string;
      blockers: string;
      plans: string;
      policies: string;
      acks: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM pricing_stage5_runs_v2) AS runs,
        (SELECT count(*)::text FROM pricing_stage5_blockers_v2) AS blockers,
        (SELECT count(*)::text FROM pricing_release_plans_v2) AS plans,
        (SELECT count(*)::text FROM pricing_policy_documents_v2) AS policies,
        (SELECT count(*)::text FROM pricing_stage5_prepare_acks_v2) AS acks
    `);
    expect(counts.rows[0]).toMatchObject({
      runs: "1",
      plans: "0",
      policies: "0",
      acks: "0",
    });
    expect(Number(counts.rows[0]!.blockers)).toBeGreaterThanOrEqual(2);
    expect(authorities.prepared.catalogs.size).toBe(0);
    expect(authorities.prepared.switches.size).toBe(0);
    expect(authorities.prepared.policies.size).toBe(0);
  });
});
