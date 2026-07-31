import { randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  buildStage5AssignmentMatrix,
  createDatabase,
  planStage5Backfill,
  runStage5Backfill,
  Stage5BackfillError,
  type Stage5Inventory,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("Stage 5 multi-discount backfill", () => {
  let admin: Client;
  let seedClient: Client;
  let databaseName: string;
  let databaseUrl: string;

  beforeAll(async () => {
    databaseName = `stage5_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    databaseUrl = url.toString();
    seedClient = new Client({ connectionString: databaseUrl });
    await seedClient.connect();
    await migrate(drizzle(seedClient), { migrationsFolder: MIGRATIONS_FOLDER });
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await seedClient?.end();
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
    const tables = await seedClient.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    if (tables.rows.length > 0) {
      await seedClient.query(
        `TRUNCATE TABLE ${tables.rows.map((row) => quoteIdentifier(row.tablename)).join(", ")} RESTART IDENTITY CASCADE`,
      );
    }
  });

  async function seedLegacyState(): Promise<{
    inventory: Stage5Inventory;
    b2cUserId: string;
    b2bUserId: string;
    b2cRecordId: string;
    b2bRecordId: string;
    inviteIds: string[];
  }> {
    const b2cUserId = randomUUID();
    const b2bUserId = randomUUID();
    const b2cRecordId = randomUUID();
    const b2bRecordId = randomUUID();
    const inviteIds = [randomUUID(), randomUUID()];
    await seedClient.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'B2C', true, 'active'), ($3, $4, 'B2B', true, 'active')
    `, [b2cUserId, `${b2cUserId}@example.test`, b2bUserId, `${b2bUserId}@example.test`]);
    await seedClient.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start
      ) VALUES
        ($1, 'b2c', 0, 4000, now()),
        ($2, 'b2b', NULL, 5500, now())
    `, [b2cUserId, b2bUserId]);
    await seedClient.query(`
      INSERT INTO engine_accounts (
        id, user_id, engine_account_id, mult_bp, status
      ) VALUES
        ($1, $2, 'acct_stage5_b2c', 4000, 'active'),
        ($3, $4, 'acct_stage5_b2b', 5500, 'active')
    `, [b2cRecordId, b2cUserId, b2bRecordId, b2bUserId]);
    await seedClient.query(`
      INSERT INTO business_invites (
        id, token_hash, multiplier_bp, expires_at, created_by_actor
      ) VALUES
        ($1, $2, 6000, now() + interval '30 days', 'test'),
        ($3, $4, 6000, now() + interval '30 days', 'test')
    `, [inviteIds[0], `hash-${inviteIds[0]}`, inviteIds[1], `hash-${inviteIds[1]}`]);
    return {
      inventory: {
        schema_version: 1,
        engine_accounts: [
          { account_id: "acct_stage5_b2c", multiplier_bp: 4000, status: "active" },
          { account_id: "acct_stage5_b2b", multiplier_bp: 5500, status: "active" },
          { account_id: "acct_stage5_openkeys", multiplier_bp: 7000, status: "active" },
          { account_id: "acct_stage5_service", multiplier_bp: 10000, status: "active" },
          { account_id: "acct_stage5_disabled", multiplier_bp: 10000, status: "disabled" },
        ],
        openkeys_accounts: [{
          source_id: "openkeys-row-1",
          account_id: "acct_stage5_openkeys",
          multiplier_bp: 7000,
          status: "active",
          pricing_contract: "legacy",
        }],
      },
      b2cUserId,
      b2bUserId,
      b2cRecordId,
      b2bRecordId,
      inviteIds,
    };
  }

  it("dry-runs an exact Gemini-free plan without writing", async () => {
    const seeded = await seedLegacyState();
    const database = createDatabase(databaseUrl, "stage5-dry-run-test");
    try {
      const result = await runStage5Backfill(database, seeded.inventory, { mode: "dry_run" });
      expect(result.writes_committed).toBe(false);
      expect(result.plan.catalogs.map((catalog) => catalog.product_id)).toEqual(["main", "openkeys"]);
      for (const catalog of result.plan.catalogs) {
        expect(catalog.entries).toHaveLength(10);
        expect(catalog.entries.some((entry) => entry.provider_id === "gemini")).toBe(false);
      }
      expect(result.plan.capability.aliases).toEqual([{
        provider_id: "openai",
        alias_model_id: "gpt-5.6",
        canonical_model_id: "gpt-5.6-sol",
      }]);
      expect(result.plan.safe.b2c_accounts).toHaveLength(1);
      expect(result.plan.safe.b2c_accounts[0]!.effective_policy.rules).toMatchObject([
        { pricing_mode: "track", payable_multiplier_bp: 4000 },
        { pricing_mode: "track", payable_multiplier_bp: 4000 },
      ]);
      expect(result.plan.protected.b2b_accounts[0]!.effective_policy.rules).toMatchObject([
        { scope: { provider: { provider_id: "anthropic" } }, payable_multiplier_bp: 5500 },
      ]);
      expect(result.plan.protected.openkeys_accounts[0]!.effective_policy?.rules).toMatchObject([
        { rule_origin: "legacy", payable_multiplier_bp: 7000 },
        { rule_origin: "legacy", payable_multiplier_bp: 7000 },
      ]);
      expect(result.plan.protected.unresolved_engine_accounts).toEqual([
        "acct_stage5_disabled",
        "acct_stage5_service",
      ]);
      const count = await seedClient.query<{ count: string }>("SELECT count(*)::text AS count FROM product_catalog_versions");
      expect(count.rows[0]!.count).toBe("0");
    } finally {
      await database.pool.end();
    }
  });

  it("atomically applies the safe graph and replays without duplicate rows", async () => {
    const seeded = await seedLegacyState();
    const database = createDatabase(databaseUrl, "stage5-safe-test");
    try {
      const first = await runStage5Backfill(database, seeded.inventory, { mode: "safe" });
      const second = await runStage5Backfill(database, seeded.inventory, { mode: "safe" });
      expect(first.plan.plan_digest).toBe(second.plan.plan_digest);
      const counts = await seedClient.query<{
        capabilities: string;
        catalogs: string;
        catalog_entries: string;
        switches: string;
        policies: string;
        invite_bindings: string;
        account_bindings: string;
        catalog_jobs: string;
        switch_jobs: string;
        policy_jobs: string;
      }>(`
        SELECT
          (SELECT count(*)::text FROM provider_capability_versions) AS capabilities,
          (SELECT count(*)::text FROM product_catalog_versions) AS catalogs,
          (SELECT count(*)::text FROM product_catalog_entries) AS catalog_entries,
          (SELECT count(*)::text FROM provider_switch_versions) AS switches,
          (SELECT count(*)::text FROM pricing_policies) AS policies,
          (SELECT count(*)::text FROM business_invite_policy_bindings) AS invite_bindings,
          (SELECT count(*)::text FROM account_policy_bindings) AS account_bindings,
          (SELECT count(*)::text FROM engine_catalog_jobs) AS catalog_jobs,
          (SELECT count(*)::text FROM engine_switch_jobs) AS switch_jobs,
          (SELECT count(*)::text FROM engine_policy_jobs) AS policy_jobs
      `);
      expect(counts.rows[0]).toEqual({
        capabilities: "1",
        catalogs: "2",
        catalog_entries: "20",
        switches: "1",
        policies: "3",
        invite_bindings: "2",
        account_bindings: "1",
        catalog_jobs: "2",
        switch_jobs: "1",
        policy_jobs: "1",
      });
      const providers = await seedClient.query<{ provider_id: string; pricing_mode: string }>(`
        SELECT provider_id, pricing_mode
        FROM account_policy_rules
        ORDER BY provider_id
      `);
      expect(providers.rows).toEqual([
        { provider_id: "anthropic", pricing_mode: "track" },
        { provider_id: "openai", pricing_mode: "track" },
      ]);
      const invitePolicies = await seedClient.query<{ invite_id: string; invitation_policy_id: string }>(`
        SELECT invite_id::text, invitation_policy_id
        FROM business_invite_policy_bindings ORDER BY invite_id
      `);
      expect(new Set(invitePolicies.rows.map((row) => row.invitation_policy_id)).size).toBe(2);
      expect(invitePolicies.rows.map((row) => row.invite_id).sort()).toEqual([...seeded.inviteIds].sort());
    } finally {
      await database.pool.end();
    }
  });

  it("requires the exact approved matrix before protected assignments", async () => {
    const seeded = await seedLegacyState();
    const database = createDatabase(databaseUrl, "stage5-approval-test");
    try {
      await expect(runStage5Backfill(database, seeded.inventory, { mode: "approved" }))
        .rejects.toMatchObject({ code: "assignment_matrix_required" });
      const count = await seedClient.query<{ count: string }>("SELECT count(*)::text AS count FROM pricing_policies");
      expect(count.rows[0]!.count).toBe("0");
    } finally {
      await database.pool.end();
    }
  });

  it("rolls back the complete graph when an immutable authority version conflicts", async () => {
    const seeded = await seedLegacyState();
    await seedClient.query(`
      INSERT INTO provider_capability_versions (
        generation, schema_version, content_digest, source_runtime, source_revision, observed_at
      ) VALUES (1, 1, 'sha256:v1:conflicting', 'test', 'test', now())
    `);
    const database = createDatabase(databaseUrl, "stage5-rollback-test");
    try {
      await expect(runStage5Backfill(database, seeded.inventory, { mode: "safe" }))
        .rejects.toMatchObject({ code: "immutable_version_conflict" });
      const counts = await seedClient.query<{ catalogs: string; policies: string; jobs: string }>(`
        SELECT
          (SELECT count(*)::text FROM product_catalog_versions) AS catalogs,
          (SELECT count(*)::text FROM pricing_policies) AS policies,
          (SELECT count(*)::text FROM engine_catalog_jobs) AS jobs
      `);
      expect(counts.rows[0]).toEqual({ catalogs: "0", policies: "0", jobs: "0" });
    } finally {
      await database.pool.end();
    }
  });

  it("applies approved B2B and service policies but leaves OpenKeys in its bounded context", async () => {
    const seeded = await seedLegacyState();
    const database = createDatabase(databaseUrl, "stage5-protected-test");
    try {
      const plan = await planStage5Backfill(database, seeded.inventory);
      const matrix = buildStage5AssignmentMatrix(plan, {
        approved_by: "pricing-owner@example.test",
        approved_at: "2026-08-01T00:00:00+00:00",
        reason: "Reviewed exact Stage 5 test assignment matrix",
        service: [{
          account_id: "acct_stage5_service",
          product_id: "main",
          owner_id: "service:stage5-test",
          policy_id: "policy:main:service:stage5-test",
          rules: [{
            rule_id: "provider:anthropic:discount",
            scope: { provider: { provider_id: "anthropic" } },
            discount_bps: 0,
          }],
        }],
        excluded_disabled_accounts: ["acct_stage5_disabled"],
      });
      const result = await runStage5Backfill(database, seeded.inventory, {
        mode: "approved",
        assignment_matrix: matrix,
      });
      expect(result.protected_assignment_digest).toBe(matrix.content_digest);
      const bindings = await seedClient.query<{ engine_account_id: string; account_class: string }>(`
        SELECT engine_account_id, account_class
        FROM account_policy_bindings ORDER BY engine_account_id
      `);
      expect(bindings.rows).toEqual([
        { engine_account_id: "acct_stage5_b2b", account_class: "b2b" },
        { engine_account_id: "acct_stage5_b2c", account_class: "b2c" },
        { engine_account_id: "acct_stage5_service", account_class: "service" },
      ]);
      const b2bProviders = await seedClient.query<{ provider_id: string }>(`
        SELECT rule.provider_id
        FROM account_policy_rules rule
        JOIN account_policy_bindings binding ON binding.id = rule.binding_id
        WHERE binding.engine_account_id = 'acct_stage5_b2b'
      `);
      expect(b2bProviders.rows).toEqual([{ provider_id: "anthropic" }]);
      const openKeysCommerceBinding = await seedClient.query<{ count: string }>(`
        SELECT count(*)::text AS count FROM account_policy_bindings
        WHERE engine_account_id = 'acct_stage5_openkeys'
      `);
      expect(openKeysCommerceBinding.rows[0]!.count).toBe("0");
    } finally {
      await database.pool.end();
    }
  });

  it("rejects same-version source drift instead of mutating immutable policy rows", async () => {
    const seeded = await seedLegacyState();
    const database = createDatabase(databaseUrl, "stage5-drift-test");
    try {
      await runStage5Backfill(database, seeded.inventory, { mode: "safe" });
      await seedClient.query("UPDATE engine_accounts SET mult_bp = 3900 WHERE engine_account_id = 'acct_stage5_b2c'");
      await seedClient.query("UPDATE customer_profiles SET multiplier_bp = 3900 WHERE user_id = $1", [seeded.b2cUserId]);
      const changedInventory: Stage5Inventory = {
        ...seeded.inventory,
        engine_accounts: seeded.inventory.engine_accounts.map((account) =>
          account.account_id === "acct_stage5_b2c" ? { ...account, multiplier_bp: 3900 } : account),
      };
      await expect(runStage5Backfill(database, changedInventory, { mode: "safe" }))
        .rejects.toMatchObject({ code: "immutable_version_conflict" });
      const stored = await seedClient.query<{ payable_multiplier_bp: number }>(`
        SELECT rule.payable_multiplier_bp
        FROM account_policy_rules rule
        JOIN account_policy_bindings binding ON binding.id = rule.binding_id
        WHERE binding.engine_account_id = 'acct_stage5_b2c'
        ORDER BY rule.provider_id
      `);
      expect(stored.rows).toEqual([
        { payable_multiplier_bp: 4000 },
        { payable_multiplier_bp: 4000 },
      ]);
    } finally {
      await database.pool.end();
    }
  });

  it("rejects a tampered assignment manifest even when its plan hash is unchanged", async () => {
    const seeded = await seedLegacyState();
    const database = createDatabase(databaseUrl, "stage5-tamper-test");
    try {
      const plan = await planStage5Backfill(database, seeded.inventory);
      const matrix = buildStage5AssignmentMatrix(plan, {
        approved_by: "pricing-owner@example.test",
        approved_at: "2026-08-01T00:00:00+00:00",
        reason: "Reviewed exact Stage 5 test assignment matrix",
        service: [{
          account_id: "acct_stage5_service",
          product_id: "main",
          owner_id: "service:stage5-test",
          policy_id: "policy:main:service:stage5-test",
          rules: [{
            rule_id: "provider:anthropic:discount",
            scope: { provider: { provider_id: "anthropic" } },
            discount_bps: 0,
          }],
        }],
        excluded_disabled_accounts: ["acct_stage5_disabled"],
      });
      const tampered = structuredClone(matrix);
      tampered.b2b[0]!.source_multiplier_bp = 4000;
      await expect(runStage5Backfill(database, seeded.inventory, {
        mode: "approved",
        assignment_matrix: tampered,
      })).rejects.toBeInstanceOf(Stage5BackfillError);
    } finally {
      await database.pool.end();
    }
  });
}, TEST_TIMEOUT_MS);
