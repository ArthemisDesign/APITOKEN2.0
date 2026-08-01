import { randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  collectStage8CommerceEvidence,
  createDatabase,
  runStage5Backfill,
  type Stage5Inventory,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("Stage 8 commerce evidence", () => {
  let admin: Client;
  let seed: Client;
  let databaseName: string;
  let databaseUrl: string;

  beforeAll(async () => {
    databaseName = `stage8_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 12)}`;
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

  async function seedConfirmedGraph(): Promise<{ accountId: string; bindingId: string }> {
    const userId = randomUUID();
    const recordId = randomUUID();
    const accountId = "acct_stage8_b2c";
    await seed.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'Stage 8', true, 'active')
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

    const inventory: Stage5Inventory = {
      schema_version: 1,
      engine_accounts: [{ account_id: accountId, multiplier_bp: 4000, status: "active" }],
      openkeys_accounts: [],
    };
    const database = createDatabase(databaseUrl, "stage8-seed");
    try {
      await runStage5Backfill(database, inventory, { mode: "safe" });
    } finally {
      await database.pool.end();
    }

    await seed.query(`
      UPDATE engine_catalog_jobs SET
        status = 'confirmed', ack_generation = generation,
        ack_schema_version = schema_version, ack_content_digest = content_digest,
        ack_payload = '{}'::jsonb, confirmed_at = now()
    `);
    await seed.query(`
      UPDATE engine_switch_jobs SET
        status = 'confirmed', ack_generation = generation,
        ack_schema_version = schema_version, ack_content_digest = content_digest,
        ack_payload = '{}'::jsonb, confirmed_at = now()
    `);
    await seed.query(`
      UPDATE engine_policy_jobs SET
        status = 'confirmed', ack_effective_version = effective_version,
        ack_policy_version = policy_version, ack_catalog_generation = catalog_generation,
        ack_switch_generation = switch_generation, ack_schema_version = schema_version,
        ack_content_digest = content_digest, ack_payload = '{}'::jsonb, confirmed_at = now()
    `);
    await seed.query(`
      UPDATE account_policy_bindings SET
        applied_effective_version = desired_effective_version,
        applied_digest = desired_digest, sync_state = 'confirmed', last_ack_at = now()
    `);
    const binding = await seed.query<{ id: string }>(
      "SELECT id::text FROM account_policy_bindings WHERE engine_account_id = $1",
      [accountId],
    );
    return { accountId, bindingId: binding.rows[0]!.id };
  }

  it("attests an exact ACKed, classified and Gemini-free graph", async () => {
    const seeded = await seedConfirmedGraph();
    const database = createDatabase(databaseUrl, "stage8-evidence-pass");
    try {
      const first = await collectStage8CommerceEvidence(database);
      expect(first.passed).toBe(true);
      expect(first.blockers).toEqual([]);
      expect(first.heads.catalogs.map((catalog) => catalog.product_id)).toEqual(["main", "openkeys"]);
      expect(first.counts).toMatchObject({
        active_commerce_accounts: 1,
        account_classes: { b2c: 1 },
        catalog_jobs: { confirmed: 2 },
        switch_jobs: { confirmed: 1 },
        policy_jobs: { confirmed: 1 },
      });
      expect(first.evidence_digest).toMatch(/^sha256:v1:[0-9a-f]{64}$/);
      expect(JSON.stringify(first)).not.toContain(seeded.accountId);
      expect(JSON.stringify(first)).not.toContain(seeded.bindingId);
    } finally {
      await database.pool.end();
    }
  });

  it("counts and blocks an active account whose commerce profile is missing", async () => {
    await seedConfirmedGraph();
    const userId = randomUUID();
    const recordId = randomUUID();
    const engineAccountId = "acct_stage8_unclassified";
    await seed.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'Unclassified', true, 'active')
    `, [userId, `${userId}@example.test`]);
    await seed.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 4000, 'active')
    `, [recordId, userId, engineAccountId]);

    const database = createDatabase(databaseUrl, "stage8-evidence-unclassified");
    try {
      const report = await collectStage8CommerceEvidence(database);
      expect(report.passed).toBe(false);
      expect(report.counts.active_commerce_accounts).toBe(2);
      expect(report.counts.account_classes).toMatchObject({ b2c: 1, unclassified: 1 });
      expect(report.blockers.map((candidate) => candidate.code)).toContain(
        "active_commerce_account_unclassified",
      );
      expect(JSON.stringify(report)).not.toContain(recordId);
      expect(JSON.stringify(report)).not.toContain(engineAccountId);
    } finally {
      await database.pool.end();
    }
  });

  it("fails closed on a stale binding and retry backlog without leaking identities", async () => {
    await seedConfirmedGraph();
    const binding = await seed.query<{ id: string }>("SELECT id::text FROM account_policy_bindings LIMIT 1");
    await seed.query(`
      UPDATE account_policy_bindings SET sync_state = 'pending', last_ack_at = NULL,
        applied_effective_version = NULL, applied_digest = NULL
      WHERE id = $1
    `, [binding.rows[0]!.id]);
    await seed.query(`
      UPDATE engine_policy_jobs SET status = 'retry', ack_effective_version = NULL,
        ack_policy_version = NULL, ack_catalog_generation = NULL, ack_switch_generation = NULL,
        ack_schema_version = NULL, ack_content_digest = NULL, ack_payload = NULL,
        confirmed_at = NULL, next_attempt_at = now()
      WHERE binding_id = $1
    `, [binding.rows[0]!.id]);

    const database = createDatabase(databaseUrl, "stage8-evidence-blocked");
    try {
      const report = await collectStage8CommerceEvidence(database);
      expect(report.passed).toBe(false);
      expect(report.blockers.map((candidate) => candidate.code)).toEqual(expect.arrayContaining([
        "active_policy_ack_missing",
        "binding_not_fully_applied",
        "pricing_control_job_backlog_or_failure",
      ]));
      expect(JSON.stringify(report)).not.toContain(binding.rows[0]!.id);
      expect(report.blockers.every((candidate) =>
        candidate.subject_digests.every((digest) => /^sha256:v1:[0-9a-f]{64}$/.test(digest))
      )).toBe(true);
    } finally {
      await database.pool.end();
    }
  });
});
