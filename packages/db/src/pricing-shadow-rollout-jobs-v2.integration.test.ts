import { createHash, randomUUID } from "node:crypto";
import type {
  PricingCatalogSpec,
  PricingReleaseInventoryAccountV2,
  ProviderSwitchSpec,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  claimPricingShadowPolicyJobsV2,
  completePricingShadowPolicyJobV2,
  createDatabase,
  failPricingShadowPolicyJobV2,
  readPricingShadowRolloutControlV2,
  recoverStalePricingShadowPolicyJobsV2,
  stage5Digest,
  stage5V2Digest,
  stage5V2EngineIdentityDigest,
  stagePricingShadowRolloutV2,
  type Database,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const V2 = (label: string): string => stage5V2Digest("shadow-rollout-integration", label);
const V1 = (label: string): string =>
  `sha256:v1:${createHash("sha256").update(`shadow-rollout-integration${label}`).digest("hex")}`;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

const TARGET_GENERATION = 93_001;
const RECOVERY_GENERATION = 93_002;

function catalog(productId: string): PricingCatalogSpec {
  return {
    product_id: productId,
    generation: 5,
    schema_version: 1,
    capability_generation: 5,
    capability_digest: stage5Digest("capability", 5),
    content_digest: stage5Digest("catalog", productId),
    entries: [
      { provider_id: "anthropic", canonical_model_id: "claude-sonnet", enabled: true },
      { provider_id: "openai", canonical_model_id: "gpt-5", enabled: true },
    ],
  };
}

function switches(): ProviderSwitchSpec {
  const entries: ProviderSwitchSpec["entries"] = [];
  for (const providerId of ["anthropic", "openai"] as const) {
    entries.push(
      { provider_id: providerId, scope: "master", catalog_generation: null, enabled: true },
      { provider_id: providerId, scope: { product: { product_id: "main" } }, catalog_generation: 5, enabled: true },
      { provider_id: providerId, scope: { product: { product_id: "openkeys" } }, catalog_generation: 5, enabled: true },
      { provider_id: providerId, scope: { segment: { product_id: "main", segment: "b2c" } }, catalog_generation: 5, enabled: true },
      { provider_id: providerId, scope: { segment: { product_id: "main", segment: "b2b" } }, catalog_generation: 5, enabled: true },
    );
  }
  return {
    generation: 5,
    schema_version: 1,
    capability_generation: 5,
    capability_digest: stage5Digest("capability", 5),
    content_digest: stage5Digest("switches", 5),
    entries,
  };
}

const LEGACY_SOURCE_ID = "3f6b1f24-9f32-4a2f-9b6e-1a2b3c4d5e6f";

function engineAccounts(): PricingReleaseInventoryAccountV2[] {
  const account = (
    accountId: string,
    multiplierBp: number,
  ): PricingReleaseInventoryAccountV2 => ({
    account_id: accountId,
    status: "active",
    multiplier_bp: multiplierBp,
    balance_nano: "1000000000",
    reserved_nano: "0",
    spent_nano: "0",
    funding_generation: 1,
    funding_head_version: 1,
  });
  return [
    account("acct_b2c", 5_000),
    account("acct_b2b", 8_000),
    account("acct_ok_legacy", 7_000),
    account("acct_ok_new", 10_000),
    account("acct_svc", 10_000),
  ].sort((left, right) => left.account_id.localeCompare(right.account_id));
}

function engineReader(
  accounts = engineAccounts(),
): Pick<EngineClient, "getPricingReleaseInventoryV2" | "getAccountPricingState"> {
  return {
    getPricingReleaseInventoryV2: async () => ({
      accounts,
      next_after_account_id: null,
    }),
    getAccountPricingState: async (accountId: string) => {
      if (accountId === "acct_ok_legacy") {
        const base = {
          account_id: "acct_ok_legacy",
          effective_version: 1,
          policy_id: `policy:openkeys:legacy:${LEGACY_SOURCE_ID}`,
          policy_version: 1,
          source_policy_digest: "sha256:v1:legacy-source",
          owner_type: "open_keys" as const,
          owner_id: LEGACY_SOURCE_ID,
          account_class: "open_keys" as const,
          product_id: "openkeys",
          schema_version: 1,
          catalog_generation: 1,
          switch_generation: 1,
          replacement_locked: true,
          rules: [
            {
              rule_id: "provider:anthropic:legacy",
              rule_digest: "sha256:v1:rule-a",
              scope: { provider: { provider_id: "anthropic" } },
              pricing_mode: "discount" as const,
              rule_origin: "legacy" as const,
              discount_bps: null,
              payable_multiplier_bp: 7_000,
              track_eligible: false,
              retention_eligible: false,
              commission_eligible: false,
            },
          ],
        };
        return {
          active: {
            policy: { ...base, content_digest: V1("legacy-v1") },
            binding: {
              policy_enforcement: "shadow" as const,
              funding_enforcement: "legacy_single" as const,
              reconciliation_state: "pending" as const,
            },
          },
        };
      }
      if (accountId !== "acct_ok_new") return "unbound" as const;
      const base = {
        account_id: "acct_ok_new",
        effective_version: 1,
        policy_id: "policy:openkeys:official-1-to-1",
        policy_version: 1,
        source_policy_digest: "sha256:v1:official-source",
        owner_type: "open_keys" as const,
        owner_id: "7c8d9e2f-1111-4a2f-9b6e-1a2b3c4d5e6f",
        account_class: "open_keys" as const,
        product_id: "openkeys",
        schema_version: 1,
        catalog_generation: 1,
        switch_generation: 1,
        replacement_locked: false,
        rules: [
          {
            rule_id: "provider:anthropic:official-1-to-1",
            rule_digest: "sha256:v1:rule-anthropic",
            scope: { provider: { provider_id: "anthropic" } },
            pricing_mode: "discount" as const,
            rule_origin: "managed" as const,
            discount_bps: 0,
            payable_multiplier_bp: 10_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
          },
        ],
      };
      return {
        active: {
          policy: { ...base, content_digest: V1("official-v1") },
          binding: {
            policy_enforcement: "shadow" as const,
            funding_enforcement: "legacy_single" as const,
            reconciliation_state: "verified" as const,
          },
        },
      };
    },
  };
}

interface PolicySeed {
  policyId: string;
  ownerType: string;
  ownerId: string;
  accountClass: string;
  productId: string | null;
  billingMode: string;
  catalogGeneration: number | null;
  rules: Array<{
    ruleId: string;
    scopeType: "global" | "provider" | "model";
    providerId: string | null;
    modelId: string | null;
    discountBps: number;
    multiplierBp: number;
  }>;
}

function policySeeds(): PolicySeed[] {
  return [
    {
      policyId: "release-v2:global-b2c",
      ownerType: "global_b2c",
      ownerId: "global-b2c",
      accountClass: "b2c",
      productId: "main",
      billingMode: "balance",
      catalogGeneration: 5,
      rules: [{
        ruleId: "global-b2c-50",
        scopeType: "global",
        providerId: null,
        modelId: null,
        discountBps: 5_000,
        multiplierBp: 5_000,
      }],
    },
    {
      policyId: "release-v2:b2b:user-1",
      ownerType: "b2b_client",
      ownerId: "user-1",
      accountClass: "b2b",
      productId: "main",
      billingMode: "balance",
      catalogGeneration: 5,
      rules: [{
        ruleId: "b2b-anthropic",
        scopeType: "provider",
        providerId: "anthropic",
        modelId: null,
        discountBps: 2_000,
        multiplierBp: 8_000,
      }],
    },
    {
      policyId: `release-v2:openkeys:${LEGACY_SOURCE_ID}`,
      ownerType: "openkeys",
      ownerId: LEGACY_SOURCE_ID,
      accountClass: "openkeys",
      productId: "openkeys",
      billingMode: "balance",
      catalogGeneration: 5,
      rules: [
        { ruleId: "ok-anthropic", scopeType: "provider", providerId: "anthropic", modelId: null, discountBps: 0, multiplierBp: 10_000 },
        { ruleId: "ok-openai", scopeType: "provider", providerId: "openai", modelId: null, discountBps: 0, multiplierBp: 10_000 },
      ],
    },
    {
      policyId: "release-v2:openkeys:new-source",
      ownerType: "openkeys",
      ownerId: "new-source",
      accountClass: "openkeys",
      productId: "openkeys",
      billingMode: "balance",
      catalogGeneration: 5,
      rules: [
        { ruleId: "ok-anthropic", scopeType: "provider", providerId: "anthropic", modelId: null, discountBps: 0, multiplierBp: 10_000 },
        { ruleId: "ok-openai", scopeType: "provider", providerId: "openai", modelId: null, discountBps: 0, multiplierBp: 10_000 },
      ],
    },
    {
      policyId: "release-v2:service:svc-1",
      ownerType: "service",
      ownerId: "svc-1",
      accountClass: "service",
      productId: null,
      billingMode: "meter_only",
      catalogGeneration: null,
      rules: [],
    },
  ];
}

function policyDigest(seed: PolicySeed): string {
  return V2(`policy:${seed.policyId}`);
}

async function seedStage5(seed: Client, runId: string): Promise<void> {
  const inventoryDigest = stage5V2EngineIdentityDigest(engineAccounts());
  const main = catalog("main");
  const openkeys = catalog("openkeys");
  const switchSpec = switches();
  const inventoryArtifact = {
    openkeys: {
      accounts: [
        {
          account_id: "acct_ok_legacy",
          source_id: LEGACY_SOURCE_ID,
          lifecycle: "active",
          pricing_contract: "legacy",
          source_multiplier_bp: 7_000,
          content_digest: V2("openkeys-legacy"),
        },
        {
          account_id: "acct_ok_new",
          source_id: "7c8d9e2f-1111-4a2f-9b6e-1a2b3c4d5e6f",
          lifecycle: "active",
          pricing_contract: "official_1_to_1",
          source_multiplier_bp: 10_000,
          content_digest: V2("openkeys-new"),
        },
      ],
    },
  };
  const planArtifact = { catalogs: [main, openkeys], switches: switchSpec };
  await seed.query(`
    INSERT INTO pricing_stage5_runs_v2 (
      run_id, schema_version, plan_digest, commerce_inventory_digest,
      engine_scan_first_digest, engine_scan_second_digest,
      openkeys_scan_first_digest, openkeys_scan_second_digest,
      service_inventory_digest, funding_plan_digest,
      target_generation, target_digest, recovery_generation, recovery_digest,
      inventory_artifact, plan_artifact, blocker_count, status
    ) VALUES ($1, 2, $2, $3, $4, $4, $5, $5, $6, $7, $8, $9, $10, $11, $12::jsonb, $13::jsonb, 0, 'prepared')
  `, [
    runId,
    V2("plan"),
    V2("commerce"),
    inventoryDigest,
    V2("openkeys-scan"),
    V2("service"),
    V2("funding-plan"),
    TARGET_GENERATION,
    V2("target-plan"),
    RECOVERY_GENERATION,
    V2("recovery-plan"),
    JSON.stringify(inventoryArtifact),
    JSON.stringify(planArtifact),
  ]);
  for (const [generation, kind, contentDigest] of [
    [TARGET_GENERATION, "target", V2("target-plan")],
    [RECOVERY_GENERATION, "recovery", V2("recovery-plan")],
  ] as const) {
    await seed.query(`
      INSERT INTO pricing_release_plans_v2 (
        generation, release_kind, schema_version,
        commerce_inventory_digest, engine_inventory_digest,
        openkeys_inventory_digest, service_inventory_digest,
        policy_manifest_digest, assignment_manifest_digest,
        funding_manifest_digest, engine_release_digest, content_digest, status
      ) VALUES ($1, $2, 2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'materializing')
    `, [
      generation,
      kind,
      V2("commerce"),
      inventoryDigest,
      V2("openkeys-scan"),
      V2("service"),
      V2(`policy-manifest-${kind}`),
      V2(`assignment-manifest-${kind}`),
      V2(`funding-manifest-${kind}`),
      V2(`engine-release-${kind}`),
      contentDigest,
    ]);
  }
  for (const policy of policySeeds()) {
    await seed.query(`
      INSERT INTO pricing_policy_documents_v2 (
        policy_id, policy_version, owner_type, owner_id, account_class,
        product_id, billing_mode, schema_version, capability_generation,
        capability_digest, catalog_generation, catalog_digest,
        switch_generation, switch_digest, content_digest
      ) VALUES ($1, 2, $2, $3, $4, $5, $6, 2, 5, $7, $8, $9, $10, $11, $12)
    `, [
      policy.policyId,
      policy.ownerType,
      policy.ownerId,
      policy.accountClass,
      policy.productId,
      policy.billingMode,
      stage5Digest("capability", 5),
      policy.catalogGeneration,
      policy.catalogGeneration === null ? null : catalog(policy.productId ?? "main").content_digest,
      policy.catalogGeneration === null ? null : 5,
      policy.catalogGeneration === null ? null : switchSpec.content_digest,
      policyDigest(policy),
    ]);
    for (const rule of policy.rules) {
      await seed.query(`
        INSERT INTO pricing_policy_rules_v2 (
          policy_id, policy_version, rule_id, rule_digest, scope_type,
          provider_id, canonical_model_id, discount_bps, payable_multiplier_bp
        ) VALUES ($1, 2, $2, $3, $4, $5, $6, $7, $8)
      `, [
        policy.policyId,
        rule.ruleId,
        V2(`rule:${policy.policyId}:${rule.ruleId}`),
        rule.scopeType,
        rule.providerId,
        rule.modelId,
        rule.discountBps,
        rule.multiplierBp,
      ]);
    }
  }
  const assignments: Array<[string, string, string, string, string, string | null, string | null]> = [
    ["acct_b2c", "b2c", "commerce", "user-b2c", "release-v2:global-b2c", null, null],
    ["acct_b2b", "b2b", "commerce", "user-1", "release-v2:b2b:user-1", null, null],
    ["acct_ok_legacy", "openkeys", "openkeys", LEGACY_SOURCE_ID, `release-v2:openkeys:${LEGACY_SOURCE_ID}`, null, null],
    ["acct_ok_new", "openkeys", "openkeys", "new-source", "release-v2:openkeys:new-source", null, null],
    ["acct_svc", "service", "service", "svc-1", "release-v2:service:svc-1", "stage7 shadow rollout", "ops-team"],
  ];
  for (const generation of [TARGET_GENERATION, RECOVERY_GENERATION]) {
    for (const [accountId, accountClass, ownerContext, ownerId, policyId, purpose, responsible] of assignments) {
      const policy = policySeeds().find((candidate) => candidate.policyId === policyId)!;
      await seed.query(`
        INSERT INTO pricing_release_assignments_v2 (
          release_generation, engine_account_id, account_class, owner_context,
          owner_id, policy_id, policy_version, policy_digest,
          billing_mode, funding_generation, purpose, responsible, assignment_digest
        ) VALUES ($1, $2, $3, $4, $5, $6, 2, $7, $8, $12, $9, $10, $11)
      `, [
        generation,
        accountId,
        accountClass,
        ownerContext,
        ownerId,
        policyId,
        policyDigest(policy),
        policy.billingMode,
        purpose,
        responsible,
        V2(`assignment:${accountId}`),
        policy.billingMode === "balance" ? 1 : null,
      ]);
      if (policy.billingMode === "balance") {
        await seed.query(`
          INSERT INTO pricing_funding_normalizations_v2 (
            release_generation, engine_account_id, funding_generation,
            expected_source_digest, target_funding_digest, applied_funding_digest,
            normalization_source, status
          ) VALUES ($1, $2, 1, $3, $4, $4, 'aggregate_paid_only', 'ready')
        `, [
          generation,
          accountId,
          V2(`source:${accountId}`),
          V2(`funding:${accountId}`),
        ]);
      }
    }
  }
  await seed.query(`
    UPDATE pricing_release_plans_v2 SET status = 'prepared', updated_at = now()
    WHERE generation = ANY($1::bigint[])
  `, [[TARGET_GENERATION, RECOVERY_GENERATION]]);
}

function stageInput(runId: string, idempotencyKey = randomUUID()) {
  return {
    idempotencyKey,
    stage5RunId: runId,
    actorId: "pricing-operator@example.test",
    reason: "align every account shadow policy before cutover",
  };
}

describe.runIf(Boolean(connectionString))("pricing shadow rollout v2 lane", () => {
  let admin: Client;
  let seed: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `shadowrollout_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seed = new Client({ connectionString: url.toString() });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "shadow-rollout-test");
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

  it("stages one exact rollout with a locked transition and a lineage advance", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const input = stageInput(runId);
    const staged = await stagePricingShadowRolloutV2(database, engineReader(), input);
    expect(staged.idempotentReplay).toBe(false);
    expect(staged.jobCount).toBe(2);

    const jobs = await seed.query<{
      engine_account_id: string;
      expected_active_version: string | null;
      expected_active_digest: string | null;
      request_payload: { kind: string; policy?: { policy_id: string; effective_version: number } };
    }>(`
      SELECT engine_account_id, expected_active_version::text, expected_active_digest, request_payload
      FROM pricing_shadow_policy_jobs_v2
      ORDER BY engine_account_id COLLATE "C"
    `);
    expect(jobs.rows.map((row) => row.engine_account_id)).toEqual([
      "acct_ok_legacy",
      "acct_ok_new",
    ]);
    const byAccount = new Map(jobs.rows.map((row) => [row.engine_account_id, row]));
    expect(byAccount.get("acct_ok_legacy")!.request_payload.kind).toBe("locked_openkeys_transition");
    expect(byAccount.get("acct_ok_legacy")!.expected_active_version).toBe("1");
    expect(byAccount.get("acct_ok_legacy")!.expected_active_digest).toBe(V1("legacy-v1"));

    // The canonical OpenKeys account advances its existing lineage in place at the next version.
    const advance = byAccount.get("acct_ok_new")!;
    expect(advance.request_payload.kind).toBe("policy_shadow");
    expect(advance.request_payload.policy!.policy_id).toBe("policy:openkeys:official-1-to-1");
    expect(advance.request_payload.policy!.effective_version).toBe(2);
    expect(advance.expected_active_version).toBe("1");
    expect(advance.expected_active_digest).toBe(V1("official-v1"));

    const again = await stagePricingShadowRolloutV2(database, engineReader(), input);
    expect(again).toEqual({ ...staged, idempotentReplay: true });
    const sameContent = await stagePricingShadowRolloutV2(
      database,
      engineReader(),
      stageInput(runId),
    );
    expect(sameContent.idempotentReplay).toBe(true);
    expect(sameContent.rolloutId).toBe(staged.rolloutId);
    await expect(stagePricingShadowRolloutV2(database, engineReader(), {
      ...input,
      stage5RunId: randomUUID(),
    })).rejects.toMatchObject({ permanent: true });

    const counts = await seed.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_shadow_rollouts_v2) AS rollouts,
        (SELECT count(*)::int FROM pricing_shadow_policy_jobs_v2) AS jobs,
        (SELECT count(*)::int FROM audit_log WHERE action = 'pricing_shadow_rollout_staged') AS audits
    `);
    expect(counts.rows[0]).toEqual({ rollouts: 1, jobs: 2, audits: 1 });
  });

  it("fails closed when the engine inventory drifted from the Stage 5 run", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const drifted = [
      ...engineAccounts(),
      { ...engineAccounts()[0]!, account_id: "acct_zzz_extra" },
    ];
    await expect(stagePricingShadowRolloutV2(
      database,
      engineReader(drifted),
      stageInput(runId),
    )).rejects.toMatchObject({ permanent: true });
    const stored = await seed.query(`SELECT count(*)::int AS count FROM pricing_shadow_rollouts_v2`);
    expect(stored.rows[0]!.count).toBe(0);
  });

  it("confirms every job and closes the rollout as confirmed", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const staged = await stagePricingShadowRolloutV2(database, engineReader(), stageInput(runId));

    const claimed = await claimPricingShadowPolicyJobsV2(database, "worker-a", {
      batchSize: 10,
      leaseMs: 300_000,
      maxAttempts: 3,
    });
    expect(claimed).toHaveLength(2);
    await expect(claimPricingShadowPolicyJobsV2(database, "worker-b", {
      batchSize: 10,
      leaseMs: 300_000,
      maxAttempts: 3,
    })).resolves.toEqual([]);

    for (const job of claimed) {
      const ackDigest = await completePricingShadowPolicyJobV2(database, job, "worker-a", {
        result: "applied",
        request_digest: job.requestDigest,
      });
      expect(ackDigest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
    }
    const rollout = await seed.query(`
      SELECT status, completed_at IS NOT NULL AS completed, last_error
      FROM pricing_shadow_rollouts_v2 WHERE id = $1
    `, [staged.rolloutId]);
    expect(rollout.rows[0]).toEqual({ status: "confirmed", completed: true, last_error: null });

    const control = await readPricingShadowRolloutControlV2(database);
    expect(control.countsByStatus.confirmed).toBe(1);
    expect(control.rollouts).toHaveLength(1);
    expect(control.rollouts[0]!.jobCountsByStatus).toEqual({ confirmed: 2 });
    expect(control.jobs).toHaveLength(2);
    for (const job of control.jobs) {
      expect(job.subjectDigest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
      expect(job).not.toHaveProperty("engineAccountId");
      expect(job.ackDigest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
    }
  });

  it("replays a terminal rollout by identity even after its lineages advanced", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const input = stageInput(runId);
    const staged = await stagePricingShadowRolloutV2(database, engineReader(), input);

    // The confirmed jobs advance the live engine lineages, so the recomputed rollout digest
    // legitimately differs from the stored one.
    const advanced = engineReader();
    const originalState = advanced.getAccountPricingState;
    advanced.getAccountPricingState = async (accountId: string) => {
      const state = await originalState(accountId);
      if (accountId === "acct_ok_new" && typeof state === "object" && state !== null && "active" in state) {
        state.active.policy.effective_version += 1;
        state.active.policy.content_digest = V1("advanced-after-confirm");
      }
      return state;
    };

    // While the rollout is still in flight, the same key with drifted recomputed content is a
    // conflict, not a replay.
    await expect(stagePricingShadowRolloutV2(database, advanced, input))
      .rejects.toMatchObject({ permanent: true, code: "idempotency_conflict" });

    const claimed = await claimPricingShadowPolicyJobsV2(database, "worker-a", {
      batchSize: 10,
      leaseMs: 300_000,
      maxAttempts: 3,
    });
    for (const job of claimed) {
      await completePricingShadowPolicyJobV2(database, job, "worker-a", {
        result: "applied",
        request_digest: job.requestDigest,
      });
    }
    const terminal = await seed.query<{ status: string }>(
      "SELECT status FROM pricing_shadow_rollouts_v2 WHERE id = $1",
      [staged.rolloutId],
    );
    expect(terminal.rows[0]!.status).toBe("confirmed");

    // A terminal rollout replays by identity and returns its stored digest.
    const replay = await stagePricingShadowRolloutV2(database, advanced, input);
    expect(replay.idempotentReplay).toBe(true);
    expect(replay.rolloutId).toBe(staged.rolloutId);
    expect(replay.rolloutDigest).toBe(staged.rolloutDigest);
  });

  it("skips a legacy-contract account whose lineage already holds the canonical successor", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const reader = engineReader();
    const originalState = reader.getAccountPricingState;
    reader.getAccountPricingState = async (accountId: string) => {
      if (accountId !== "acct_ok_legacy") return originalState(accountId);
      const base = {
        account_id: "acct_ok_legacy",
        effective_version: 2,
        policy_id: `policy:openkeys:legacy:${LEGACY_SOURCE_ID}`,
        policy_version: 2,
        source_policy_digest: V1("legacy-v1"),
        owner_type: "open_keys" as const,
        owner_id: LEGACY_SOURCE_ID,
        account_class: "open_keys" as const,
        product_id: "openkeys",
        schema_version: 1,
        catalog_generation: 5,
        switch_generation: 5,
        replacement_locked: false,
        rules: [
          {
            rule_id: "openkeys-anthropic-1to1",
            rule_digest: "sha256:v2:unused",
            scope: { provider: { provider_id: "anthropic" } },
            pricing_mode: "discount" as const,
            rule_origin: "managed" as const,
            discount_bps: 0,
            payable_multiplier_bp: 10_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
          },
          {
            rule_id: "openkeys-openai-1to1",
            rule_digest: "sha256:v2:unused2",
            scope: { provider: { provider_id: "openai" } },
            pricing_mode: "discount" as const,
            rule_origin: "managed" as const,
            discount_bps: 0,
            payable_multiplier_bp: 10_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
          },
        ],
      };
      return {
        active: {
          policy: { ...base, content_digest: V1("successor-v2") },
          binding: {
            policy_enforcement: "shadow" as const,
            funding_enforcement: "legacy_single" as const,
            reconciliation_state: "verified" as const,
          },
        },
      };
    };
    const staged = await stagePricingShadowRolloutV2(database, reader, stageInput(runId));
    expect(staged.jobCount).toBe(1);
    const jobs = await seed.query<{ engine_account_id: string }>(`
      SELECT engine_account_id FROM pricing_shadow_policy_jobs_v2
    `);
    expect(jobs.rows.map((row) => row.engine_account_id)).toEqual(["acct_ok_new"]);
  });

  it("skips a legacy-contract account whose lineage already carries the release-v2 target shape", async () => {
    // A lineage advanced once through the generic CAS lane carries the release-v2 target rules
    // converted from the target document (here the fixture's provider-scoped `ok-*` rules), not
    // the v1 transition shape. Staging must recognize it as canonical instead of failing
    // openkeys_lock_drift.
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const reader = engineReader();
    const originalState = reader.getAccountPricingState;
    reader.getAccountPricingState = async (accountId: string) => {
      if (accountId !== "acct_ok_legacy") return originalState(accountId);
      const targetRule = (providerId: string) => ({
        rule_id: `ok-${providerId}`,
        rule_digest: "sha256:v2:unused",
        scope: { provider: { provider_id: providerId } },
        pricing_mode: "discount" as const,
        rule_origin: "managed" as const,
        discount_bps: 0,
        payable_multiplier_bp: 10_000,
        track_eligible: false,
        retention_eligible: false,
        commission_eligible: false,
      });
      return {
        active: {
          policy: {
            account_id: "acct_ok_legacy",
            effective_version: 3,
            policy_id: `policy:openkeys:legacy:${LEGACY_SOURCE_ID}`,
            policy_version: 3,
            source_policy_digest: V1("legacy-v1"),
            owner_type: "open_keys" as const,
            owner_id: LEGACY_SOURCE_ID,
            account_class: "open_keys" as const,
            product_id: "openkeys",
            schema_version: 1,
            catalog_generation: 5,
            switch_generation: 5,
            replacement_locked: false,
            content_digest: V1("advanced-v3"),
            rules: [targetRule("anthropic"), targetRule("openai")],
          },
          binding: {
            policy_enforcement: "shadow" as const,
            funding_enforcement: "legacy_single" as const,
            reconciliation_state: "verified" as const,
          },
        },
      };
    };
    const staged = await stagePricingShadowRolloutV2(database, reader, stageInput(runId));
    expect(staged.jobCount).toBe(1);
    const jobs = await seed.query<{ engine_account_id: string }>(`
      SELECT engine_account_id FROM pricing_shadow_policy_jobs_v2
    `);
    expect(jobs.rows.map((row) => row.engine_account_id)).toEqual(["acct_ok_new"]);
  });

  it("advances an unlocked legacy successor whose catalog pins are stale", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const reader = engineReader();
    const originalState = reader.getAccountPricingState;
    reader.getAccountPricingState = async (accountId: string) => {
      if (accountId !== "acct_ok_legacy") return originalState(accountId);
      const policy = {
        account_id: "acct_ok_legacy",
        effective_version: 2,
        policy_id: `policy:openkeys:legacy:${LEGACY_SOURCE_ID}`,
        policy_version: 2,
        source_policy_digest: V1("legacy-v1"),
        owner_type: "open_keys" as const,
        owner_id: LEGACY_SOURCE_ID,
        account_class: "open_keys" as const,
        product_id: "openkeys",
        schema_version: 1,
        catalog_generation: 4,
        switch_generation: 4,
        content_digest: V1("successor-v2-stale"),
        replacement_locked: false,
        rules: [
          {
            rule_id: "openkeys-anthropic-1to1",
            rule_digest: "sha256:v2:stale-a",
            scope: { provider: { provider_id: "anthropic" } },
            pricing_mode: "discount" as const,
            rule_origin: "managed" as const,
            discount_bps: 0,
            payable_multiplier_bp: 10_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
          },
          {
            rule_id: "openkeys-openai-1to1",
            rule_digest: "sha256:v2:stale-b",
            scope: { provider: { provider_id: "openai" } },
            pricing_mode: "discount" as const,
            rule_origin: "managed" as const,
            discount_bps: 0,
            payable_multiplier_bp: 10_000,
            track_eligible: false,
            retention_eligible: false,
            commission_eligible: false,
          },
        ],
      };
      return {
        active: {
          policy,
          binding: {
            policy_enforcement: "shadow" as const,
            funding_enforcement: "legacy_single" as const,
            reconciliation_state: "verified" as const,
          },
        },
      };
    };

    const staged = await stagePricingShadowRolloutV2(database, reader, stageInput(runId));
    expect(staged.jobCount).toBe(2);
    const stored = await seed.query<{
      request_payload: {
        kind: string;
        policy: {
          policy_id: string;
          policy_version: number;
          effective_version: number;
          catalog_generation: number;
          switch_generation: number;
        };
      };
      expected_active_version: string | null;
      expected_active_digest: string | null;
    }>(`
      SELECT request_payload, expected_active_version::text, expected_active_digest
      FROM pricing_shadow_policy_jobs_v2
      WHERE engine_account_id = 'acct_ok_legacy'
    `);
    expect(stored.rows).toHaveLength(1);
    expect(stored.rows[0]).toMatchObject({
      request_payload: {
        kind: "policy_shadow",
        policy: {
          policy_id: `policy:openkeys:legacy:${LEGACY_SOURCE_ID}`,
          policy_version: 3,
          effective_version: 3,
          catalog_generation: 5,
          switch_generation: 5,
        },
      },
      expected_active_version: "2",
      expected_active_digest: V1("successor-v2-stale"),
    });
  });

  it("rejects an unlocked legacy lineage without canonical 1:1 semantics", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const reader = engineReader();
    const originalState = reader.getAccountPricingState;
    reader.getAccountPricingState = async (accountId: string) => {
      if (accountId !== "acct_ok_legacy") return originalState(accountId);
      const state = await originalState(accountId);
      if (state === "unbound" || "inactive" in state) throw new Error("legacy fixture is unbound");
      return {
        active: {
          ...state.active,
          policy: {
            ...state.active.policy,
            replacement_locked: false,
          },
        },
      };
    };

    await expect(stagePricingShadowRolloutV2(database, reader, stageInput(runId)))
      .rejects.toMatchObject({ permanent: true, code: "openkeys_lock_drift" });
    const stored = await seed.query(`SELECT count(*)::int AS count FROM pricing_shadow_rollouts_v2`);
    expect(stored.rows[0]!.count).toBe(0);
  });

  it("blocks semantic failures and closes the rollout as blocked", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    const staged = await stagePricingShadowRolloutV2(database, engineReader(), stageInput(runId));
    const claimed = await claimPricingShadowPolicyJobsV2(database, "worker-a", {
      batchSize: 10,
      leaseMs: 300_000,
      maxAttempts: 3,
    });
    const [first, ...rest] = claimed;
    const status = await failPricingShadowPolicyJobV2(
      database,
      first!,
      "worker-a",
      "blocked",
      "engine rejected the transition with policy_cas_mismatch",
      { retryMs: 15_000, maxAttempts: 3 },
    );
    expect(status).toBe("blocked");
    for (const job of rest) {
      await completePricingShadowPolicyJobV2(database, job, "worker-a", { result: "unchanged" });
    }
    const rollout = await seed.query(`
      SELECT status, last_error FROM pricing_shadow_rollouts_v2 WHERE id = $1
    `, [staged.rolloutId]);
    expect(rollout.rows[0]!.status).toBe("blocked");
    expect(rollout.rows[0]!.last_error).toContain("blocked");
  });

  it("retries transient failures, expires attempts into dead and reclaims expired leases", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    await stagePricingShadowRolloutV2(database, engineReader(), stageInput(runId));
    const claimed = await claimPricingShadowPolicyJobsV2(database, "worker-a", {
      batchSize: 1,
      leaseMs: 300_000,
      maxAttempts: 2,
    });
    expect(claimed).toHaveLength(1);
    const job = claimed[0]!;
    const retried = await failPricingShadowPolicyJobV2(
      database,
      job,
      "worker-a",
      "retry",
      "engine transport timeout",
      { retryMs: 1, maxAttempts: 2 },
    );
    expect(retried).toBe("retry");
    await seed.query(`
      UPDATE pricing_shadow_policy_jobs_v2 SET next_attempt_at = now() - interval '1 second'
      WHERE id = $1
    `, [job.id]);

    const reclaimed = await claimPricingShadowPolicyJobsV2(database, "worker-a", {
      batchSize: 1,
      leaseMs: 300_000,
      maxAttempts: 2,
    });
    expect(reclaimed).toHaveLength(1);
    expect(reclaimed[0]!.attempts).toBe(2);

    await seed.query(`
      UPDATE pricing_shadow_policy_jobs_v2
      SET locked_at = now() - interval '1 hour'
      WHERE id = $1
    `, [job.id]);
    const recovered = await recoverStalePricingShadowPolicyJobsV2(database, 1_000, 2);
    expect(recovered).toBe(1);
    const expired = await seed.query(`
      SELECT status, completed_at IS NOT NULL AS completed, last_error
      FROM pricing_shadow_policy_jobs_v2 WHERE id = $1
    `, [job.id]);
    expect(expired.rows[0]).toMatchObject({ status: "dead", completed: true });
  });

  it("rejects completion with a stale lease predicate", async () => {
    const runId = randomUUID();
    await seedStage5(seed, runId);
    await stagePricingShadowRolloutV2(database, engineReader(), stageInput(runId));
    const claimed = await claimPricingShadowPolicyJobsV2(database, "worker-a", {
      batchSize: 1,
      leaseMs: 300_000,
      maxAttempts: 3,
    });
    const job = claimed[0]!;
    await expect(completePricingShadowPolicyJobV2(
      database,
      job,
      "worker-b",
      { result: "applied" },
    )).rejects.toMatchObject({ permanent: false });
    const stored = await seed.query(`
      SELECT status FROM pricing_shadow_policy_jobs_v2 WHERE id = $1
    `, [job.id]);
    expect(stored.rows[0]!.status).toBe("processing");
  });
});
